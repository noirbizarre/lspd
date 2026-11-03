//! LSP *activation* — which named definitions are active for this project.
//!
//! Deliberately its own tiny module, separate from `config::lsp`, because the
//! spec's central requirement is that definitions and activation merge
//! independently (§4, §9): a user can redefine `pyright` without touching
//! which LSPs run, or change the enabled set without redefining any LSP.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::lsp::LspDefinitionPatch;
use crate::project::ProjectType;

/// The `[lsps]` table in a single configuration layer.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ActivationPatch {
    /// The full set of active LSP names, if this layer sets it at all.
    /// Setting it replaces the value from every lower layer outright — it is
    /// never appended to (spec §7: "override the active list locally").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<Vec<String>>,
}

impl ActivationPatch {
    /// Folds `patch` on top of `self`: whole-list replacement, not merging.
    #[must_use]
    pub fn merge(self, patch: Self) -> Self {
        Self {
            enabled: patch.enabled.or(self.enabled),
        }
    }
}

/// Resolves the effective set of active LSP names.
///
/// If any layer ever set `[lsps].enabled`, that value wins outright — explicit
/// configuration always beats zero-config inference, with no partial mixing
/// of the two (spec §8: registry defaults "should not force a project to
/// declare a `lspd.toml`", but an explicit list is still absolute once
/// given). Otherwise, falls back to every registry definition marked
/// `auto_enable = true` whose `languages` intersects a detected project type.
#[must_use]
pub fn resolve(
    activation: ActivationPatch,
    definitions: &HashMap<String, LspDefinitionPatch>,
    detected_types: &[ProjectType],
) -> Vec<String> {
    if let Some(enabled) = activation.enabled {
        return enabled;
    }

    let languages: Vec<&str> = detected_types.iter().map(|t| t.language()).collect();
    let mut auto_enabled: Vec<String> = definitions
        .iter()
        .filter(|(_, def)| def.auto_enable == Some(true))
        .filter(|(_, def)| {
            def.languages
                .as_ref()
                .is_some_and(|langs| langs.iter().any(|lang| languages.contains(&lang.as_str())))
        })
        .map(|(name, _)| name.clone())
        .collect();
    // Deterministic output: iteration order over a `HashMap` is not, and this
    // feeds directly into `lsp list --json`, which should not flap between
    // runs with identical input.
    auto_enabled.sort();
    auto_enabled
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::lsp::CommandSpec;

    fn def(languages: &[&str], auto_enable: bool) -> LspDefinitionPatch {
        LspDefinitionPatch {
            command: Some(CommandSpec::Executable("stub".to_string())),
            languages: Some(languages.iter().map(|s| s.to_string()).collect()),
            auto_enable: Some(auto_enable),
            ..Default::default()
        }
    }

    #[test]
    fn an_explicit_enabled_list_wins_even_when_auto_enable_candidates_exist() {
        let definitions = HashMap::from([("ruff".to_string(), def(&["python"], true))]);
        let activation = ActivationPatch {
            enabled: Some(vec!["pyright".to_string()]),
        };
        let resolved = resolve(activation, &definitions, &[ProjectType::Python]);
        assert_eq!(resolved, vec!["pyright".to_string()]);
    }

    #[test]
    fn with_no_explicit_list_both_auto_enable_python_definitions_activate_together() {
        let definitions = HashMap::from([
            ("pyright".to_string(), def(&["python"], true)),
            ("ruff".to_string(), def(&["python"], true)),
            ("gopls".to_string(), def(&["go"], true)),
        ]);
        let resolved = resolve(
            ActivationPatch::default(),
            &definitions,
            &[ProjectType::Python],
        );
        assert_eq!(resolved, vec!["pyright".to_string(), "ruff".to_string()]);
    }

    #[test]
    fn definitions_without_auto_enable_never_activate_implicitly() {
        let definitions = HashMap::from([("basedpyright".to_string(), def(&["python"], false))]);
        let resolved = resolve(
            ActivationPatch::default(),
            &definitions,
            &[ProjectType::Python],
        );
        assert_eq!(resolved, Vec::<String>::new());
    }

    #[test]
    fn a_later_layers_activation_patch_replaces_rather_than_appends() {
        let base = ActivationPatch {
            enabled: Some(vec!["pyright".to_string(), "ruff".to_string()]),
        };
        let patch = ActivationPatch {
            enabled: Some(vec!["basedpyright".to_string()]),
        };
        assert_eq!(
            base.merge(patch).enabled,
            Some(vec!["basedpyright".to_string()])
        );
    }
}
