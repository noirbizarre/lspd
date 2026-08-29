//! LSP *definitions* — the "named configuration" half of the spec's central
//! separation. `config::activation` is the other half; the two are merged
//! completely independently (spec §4, §7, §9).
//!
//! Every layer (registry, user, project, local) parses into an
//! [`LspDefinitionPatch`] — every field optional, meaning "this layer didn't
//! say". [`LspDefinitionPatch::merge`] folds patches from lowest to highest
//! priority; [`LspDefinitionPatch::resolve`] turns the final merged patch
//! into a concrete [`ResolvedLspDefinition`], failing if `command` is still
//! unset (nothing, across every layer, named an executable).

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::merge;
use crate::error::Error;

/// `command = "pyright-langserver"` or `command = ["pyright-langserver",
/// "--stdio"]` — the array form is accepted so the spec's example TOML works
/// verbatim, with any extra array elements folded into `args` (element 0 is
/// always the executable).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum CommandSpec {
    /// Just the executable; arguments come only from the `args` field.
    Executable(String),
    /// Executable plus baked-in leading arguments, e.g. from the spec's
    /// `["pyright-langserver", "--stdio"]` example.
    ExecutableWithArgs(Vec<String>),
}

impl<'de> Deserialize<'de> for CommandSpec {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Repr {
            String(String),
            Array(Vec<String>),
        }
        Ok(match Repr::deserialize(deserializer)? {
            Repr::String(s) => CommandSpec::Executable(s),
            Repr::Array(items) => CommandSpec::ExecutableWithArgs(items),
        })
    }
}

impl CommandSpec {
    /// Splits into `(executable, leading_args)`.
    #[must_use]
    pub fn split(&self) -> (&str, &[String]) {
        match self {
            Self::Executable(exe) => (exe, &[]),
            Self::ExecutableWithArgs(items) => {
                // Validated non-empty at parse time by callers; an empty
                // array here means the config asked for a definition with no
                // executable at all, which `resolve` reports as
                // `MissingCommand` rather than panicking.
                let exe = items.first().map(String::as_str).unwrap_or_default();
                (exe, &items[1..])
            }
        }
    }
}

/// Lifecycle knobs, independently overridable per field like everything
/// else in a patch.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LifecyclePatch {
    /// Restart the process automatically if it exits unexpectedly. Defaults
    /// to `true` when resolved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_restart: Option<bool>,
    /// Give up restarting after this many consecutive crashes. Defaults to
    /// `5` when resolved. `None` here (patch) vs `None` (resolved, meaning
    /// "unlimited") are different things, so resolution maps an unset patch
    /// field to `Some(5)`, not to "unlimited" — an explicit `0` is how a
    /// project asks for unlimited restarts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_restarts: Option<u32>,
    /// How long to wait for graceful shutdown before killing the process.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shutdown_timeout_ms: Option<u64>,
}

impl LifecyclePatch {
    fn merge(base: Self, patch: Self) -> Self {
        Self {
            auto_restart: merge::replace(base.auto_restart, patch.auto_restart),
            max_restarts: merge::replace(base.max_restarts, patch.max_restarts),
            shutdown_timeout_ms: merge::replace(
                base.shutdown_timeout_ms,
                patch.shutdown_timeout_ms,
            ),
        }
    }
}

/// Resolved, concrete lifecycle configuration — every field defaulted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct Lifecycle {
    /// See [`LifecyclePatch::auto_restart`].
    pub auto_restart: bool,
    /// See [`LifecyclePatch::max_restarts`]. `0` means unlimited.
    pub max_restarts: u32,
    /// See [`LifecyclePatch::shutdown_timeout_ms`].
    pub shutdown_timeout_ms: u64,
}

impl From<LifecyclePatch> for Lifecycle {
    fn from(patch: LifecyclePatch) -> Self {
        Self {
            auto_restart: patch.auto_restart.unwrap_or(true),
            max_restarts: patch.max_restarts.unwrap_or(5),
            shutdown_timeout_ms: patch.shutdown_timeout_ms.unwrap_or(5_000),
        }
    }
}

/// One `[lsp.<name>]` table as parsed from a single configuration layer.
/// `name` itself is the map key in [`super::layer::ConfigPatch::lsp`], not a
/// field here.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct LspDefinitionPatch {
    /// The executable to run, and optionally its leading arguments.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<CommandSpec>,
    /// Additional arguments, appended after any baked into `command`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<Vec<String>>,
    /// Environment variables. May contain `{{ env.* }}` templates, rendered
    /// in `config::mod` before this leaves ProjectD (spec §6).
    #[serde(default)]
    pub environment: HashMap<String, String>,
    /// Filetypes/languages this definition covers. Deliberately a list: one
    /// definition may cover several languages (spec §4).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub languages: Option<Vec<String>>,
    /// LSP `initializationOptions`, passed through verbatim to the server's
    /// `initialize` request. Kept as JSON (via [`lsp_types`]'s own
    /// passthrough shape) rather than a ProjectD-specific structure, per the
    /// spec's "preserve native LSP terminology" requirement (§5).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initialization_options: Option<serde_json::Value>,
    /// LSP workspace `settings` (`workspace/configuration`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settings: Option<serde_json::Value>,
    /// Client capability overrides merged into ProjectD's default set before
    /// `initialize`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<serde_json::Value>,
    /// Process lifecycle knobs.
    #[serde(default, skip_serializing_if = "LifecyclePatch::is_empty")]
    pub lifecycle: LifecyclePatch,
    /// Registry hint: auto-enable this definition for zero-config projects
    /// whose detected type intersects `languages` (`config::activation`).
    /// Only meaningful on registry entries; a project overriding a registry
    /// definition would not normally set this, but nothing stops it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_enable: Option<bool>,
}

impl LifecyclePatch {
    fn is_empty(&self) -> bool {
        self == &Self::default()
    }
}

impl LspDefinitionPatch {
    /// Folds `patch` on top of `self` (`self` is the lower-priority layer).
    #[must_use]
    pub fn merge(self, patch: Self) -> Self {
        Self {
            command: merge::replace(self.command, patch.command),
            args: merge::replace(self.args, patch.args),
            environment: merge::merge_env(self.environment, patch.environment),
            languages: merge::replace(self.languages, patch.languages),
            initialization_options: merge::merge_json_opt(
                self.initialization_options,
                patch.initialization_options,
            ),
            settings: merge::merge_json_opt(self.settings, patch.settings),
            capabilities: merge::merge_json_opt(self.capabilities, patch.capabilities),
            lifecycle: LifecyclePatch::merge(self.lifecycle, patch.lifecycle),
            auto_enable: merge::replace(self.auto_enable, patch.auto_enable),
        }
    }

    /// Turns a fully-merged patch into a concrete definition, or fails if no
    /// layer ever set a `command`.
    pub fn resolve(self, name: &str) -> crate::Result<ResolvedLspDefinition> {
        let command = self.command.ok_or_else(|| Error::MissingCommand {
            name: name.to_string(),
        })?;
        let (executable, leading_args) = command.split();
        if executable.is_empty() {
            return Err(Error::MissingCommand {
                name: name.to_string(),
            });
        }
        let mut args = leading_args.to_vec();
        args.extend(self.args.unwrap_or_default());

        Ok(ResolvedLspDefinition {
            name: name.to_string(),
            executable: executable.to_string(),
            args,
            environment: self.environment,
            languages: self.languages.unwrap_or_default(),
            initialization_options: self.initialization_options,
            settings: self.settings,
            capabilities: self.capabilities,
            lifecycle: self.lifecycle.into(),
            auto_enable: self.auto_enable.unwrap_or(false),
        })
    }
}

/// A fully resolved, concrete LSP definition — the effective configuration a
/// client asking `projectd lsp config <name>` receives (spec §18). Every
/// template has already been rendered by the time this exists.
#[derive(Debug, Clone, PartialEq, Serialize, schemars::JsonSchema)]
pub struct ResolvedLspDefinition {
    /// The stable logical name (the `[lsp.<name>]` key).
    pub name: String,
    /// The resolved executable path/name.
    pub executable: String,
    /// Full argument list, in invocation order.
    pub args: Vec<String>,
    /// Fully rendered environment variables to launch the process with.
    pub environment: HashMap<String, String>,
    /// Languages/filetypes this definition covers.
    pub languages: Vec<String>,
    /// Rendered `initializationOptions`, if any.
    pub initialization_options: Option<serde_json::Value>,
    /// Rendered `settings`, if any.
    pub settings: Option<serde_json::Value>,
    /// Rendered capability overrides, if any.
    pub capabilities: Option<serde_json::Value>,
    /// Resolved lifecycle configuration.
    pub lifecycle: Lifecycle,
    /// Whether the registry marked this definition for zero-config
    /// auto-enablement.
    pub auto_enable: bool,
}

impl ResolvedLspDefinition {
    /// The full command line, executable first.
    #[must_use]
    pub fn command_line(&self) -> Vec<String> {
        let mut line = Vec::with_capacity(self.args.len() + 1);
        line.push(self.executable.clone());
        line.extend(self.args.iter().cloned());
        line
    }
}

/// Path helper kept here rather than in `daemon::lsp_manager` since both the
/// daemon and `commands::lsp` need a definition's identity for logging.
#[must_use]
pub fn describe(root: &std::path::Path, name: &str) -> PathBuf {
    root.join(".projectd").join(format!("{name}.log"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn patch_with_command(command: &str) -> LspDefinitionPatch {
        LspDefinitionPatch {
            command: Some(CommandSpec::Executable(command.to_string())),
            ..Default::default()
        }
    }

    #[test]
    fn command_given_as_a_string_deserializes_to_the_executable_variant() {
        #[derive(Deserialize)]
        struct Wrapper {
            command: CommandSpec,
        }
        let parsed: Wrapper = toml::from_str("command = \"ruff\"").unwrap();
        assert_eq!(parsed.command, CommandSpec::Executable("ruff".to_string()));
    }

    #[test]
    fn command_given_as_an_array_splits_into_executable_and_leading_args() {
        let spec = CommandSpec::ExecutableWithArgs(vec![
            "pyright-langserver".to_string(),
            "--stdio".to_string(),
        ]);
        let (exe, args) = spec.split();
        assert_eq!(exe, "pyright-langserver");
        assert_eq!(args, ["--stdio"]);
    }

    #[test]
    fn a_later_layer_setting_command_replaces_the_earlier_one_wholesale() {
        let base = patch_with_command("pyright-langserver");
        let patch = patch_with_command("basedpyright-langserver");
        let merged = base.merge(patch);
        assert_eq!(
            merged.command,
            Some(CommandSpec::Executable(
                "basedpyright-langserver".to_string()
            ))
        );
    }

    #[test]
    fn a_later_layer_overriding_only_settings_keeps_the_earlier_command() {
        let base = LspDefinitionPatch {
            command: Some(CommandSpec::ExecutableWithArgs(vec![
                "pyright-langserver".to_string(),
                "--stdio".to_string(),
            ])),
            ..Default::default()
        };
        let patch = LspDefinitionPatch {
            settings: Some(json!({"python": {"analysis": {"typeCheckingMode": "strict"}}})),
            ..Default::default()
        };
        let resolved = base.merge(patch).resolve("pyright").unwrap();

        assert_eq!(resolved.executable, "pyright-langserver");
        assert_eq!(resolved.args, vec!["--stdio".to_string()]);
        assert_eq!(
            resolved.settings,
            Some(json!({"python": {"analysis": {"typeCheckingMode": "strict"}}}))
        );
    }

    #[test]
    fn environment_variables_merge_per_key_rather_than_replacing_the_whole_map() {
        let base = LspDefinitionPatch {
            environment: HashMap::from([("A".to_string(), "1".to_string())]),
            ..Default::default()
        };
        let patch = LspDefinitionPatch {
            environment: HashMap::from([("B".to_string(), "2".to_string())]),
            ..Default::default()
        };
        let merged = base.merge(patch);
        assert_eq!(
            merged.environment,
            HashMap::from([
                ("A".to_string(), "1".to_string()),
                ("B".to_string(), "2".to_string())
            ])
        );
    }

    #[test]
    fn resolving_a_patch_with_no_command_from_any_layer_fails() {
        let patch = LspDefinitionPatch {
            settings: Some(json!({"x": 1})),
            ..Default::default()
        };
        let err = patch.resolve("mystery").unwrap_err();
        assert!(matches!(err, Error::MissingCommand { name } if name == "mystery"));
    }

    #[test]
    fn lifecycle_defaults_are_sensible_when_nothing_overrides_them() {
        let resolved = patch_with_command("gopls").resolve("gopls").unwrap();
        assert!(resolved.lifecycle.auto_restart);
        assert_eq!(resolved.lifecycle.max_restarts, 5);
    }
}
