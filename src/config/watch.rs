//! Watch rules — optional consumers of the filesystem event stream.
//!
//! Kept entirely separate from the event stream itself (`daemon::events`):
//! LSPD emits events regardless of whether any watch rule exists, and a
//! watch rule is just one more subscriber that happens to run a command
//! (spec §20, §21). Rules merge by `name`, same shape as LSP definitions.

use serde::{Deserialize, Serialize};

use super::lsp::CommandSpec;
use super::merge;

/// The kinds of filesystem event a watch rule can match against. Mirrors
/// `daemon::events::FsEvent` but as a plain matching tag rather than a
/// payload-carrying event — a rule says "modified", not "modified to what".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub enum EventKind {
    /// `file.created`.
    #[serde(rename = "file.created")]
    Created,
    /// `file.modified`.
    #[serde(rename = "file.modified")]
    Modified,
    /// `file.deleted`.
    #[serde(rename = "file.deleted")]
    Deleted,
    /// `file.renamed`.
    #[serde(rename = "file.renamed")]
    Renamed,
}

/// One `[[watch]]` entry as parsed from a single layer, before merging.
/// `name` is not itself a field — see [`super::layer::load`] for how the
/// spec's `[[watch]]` array is re-keyed by `name` at load time.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct WatchRulePatch {
    /// Glob patterns (relative to the project root) this rule applies to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paths: Option<Vec<String>>,
    /// Which event kinds trigger the command.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub events: Option<Vec<EventKind>>,
    /// The command to run when a matching event occurs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<CommandSpec>,
    /// Whether the rule is active. Defaults to `true` when resolved. This is
    /// the config-file default; `daemon::watch_runtime` additionally tracks
    /// a runtime-only override set by `lspd watch enable/disable`
    /// (spec's local-override requirement), which is deliberately not
    /// persisted back into any config file (see
    /// `docs/adr/0002-config-layering-and-merge-semantics.md`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
}

impl WatchRulePatch {
    /// Folds `patch` on top of `self`. Every field is replace-if-present:
    /// unlike LSP `settings`, a watch rule's `paths`/`events` are flat lists
    /// with no natural recursive-merge shape, so redefining a rule in a
    /// higher layer is expected to restate it fully.
    #[must_use]
    pub fn merge(self, patch: Self) -> Self {
        Self {
            paths: merge::replace(self.paths, patch.paths),
            events: merge::replace(self.events, patch.events),
            command: merge::replace(self.command, patch.command),
            enabled: merge::replace(self.enabled, patch.enabled),
        }
    }

    /// Resolves into a concrete rule, or `None` if no layer ever gave it a
    /// command (an incomplete rule is silently inert rather than a hard
    /// error, since `paths`/`events` alone with no command is a config
    /// mistake that should not crash the daemon on load).
    #[must_use]
    pub fn resolve(self, name: &str) -> Option<WatchRule> {
        let command = self.command?;
        Some(WatchRule {
            name: name.to_string(),
            paths: self.paths.unwrap_or_default(),
            events: self.events.unwrap_or_else(|| vec![EventKind::Modified]),
            command,
            enabled: self.enabled.unwrap_or(true),
        })
    }
}

/// A fully resolved watch rule.
#[derive(Debug, Clone, Serialize)]
pub struct WatchRule {
    /// The rule's stable name (merge key and CLI identifier).
    pub name: String,
    /// Glob patterns it applies to.
    pub paths: Vec<String>,
    /// Event kinds it triggers on.
    pub events: Vec<EventKind>,
    /// The command to execute.
    pub command: CommandSpec,
    /// Whether it is active per configuration (before any runtime override).
    pub enabled: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_kind_parses_from_the_documented_file_dot_prefixed_spelling() {
        // Regression test: the spec's example (§21) writes
        // `events = ["file.modified"]`, not `["modified"]`. A bare
        // `rename_all = "snake_case"` compiles and round-trips against
        // itself while silently rejecting every config file written against
        // the documented schema — this parses the exact string from the
        // spec, not just any string this enum happens to produce.
        #[derive(Deserialize)]
        struct Wrapper {
            events: Vec<EventKind>,
        }
        let parsed: Wrapper = toml::from_str(r#"events = ["file.modified"]"#).unwrap();
        assert_eq!(parsed.events, vec![EventKind::Modified]);
    }

    #[test]
    fn a_rule_with_no_command_from_any_layer_resolves_to_nothing() {
        let patch = WatchRulePatch {
            paths: Some(vec!["**/*.ts".to_string()]),
            ..Default::default()
        };
        assert!(patch.resolve("incomplete").is_none());
    }

    #[test]
    fn defaults_apply_when_events_and_enabled_are_unset() {
        let patch = WatchRulePatch {
            command: Some(CommandSpec::Executable("true".to_string())),
            ..Default::default()
        };
        let rule = patch.resolve("frontend-tests").unwrap();
        assert_eq!(rule.events, vec![EventKind::Modified]);
        assert!(rule.enabled);
    }

    #[test]
    fn a_later_layer_disabling_a_rule_overrides_the_earlier_enabled_value() {
        let base = WatchRulePatch {
            command: Some(CommandSpec::Executable("true".to_string())),
            enabled: Some(true),
            ..Default::default()
        };
        let patch = WatchRulePatch {
            enabled: Some(false),
            ..Default::default()
        };
        let merged = base.merge(patch);
        assert_eq!(merged.enabled, Some(false));
    }
}
