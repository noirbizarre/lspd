//! Loading a single configuration layer (registry aside — see
//! `config::registry`) from a TOML file into the patch shape merged by
//! `config::mod`.
//!
//! A layer that does not exist is not an error: "LSPD must work when the
//! project contains no LSPD configuration" (spec §9) is satisfied here by
//! [`load`] returning an empty [`ConfigPatch`] rather than a special case the
//! caller has to remember to handle.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::activation::ActivationPatch;
use super::lsp::LspDefinitionPatch;
use super::watch::WatchRulePatch;
use crate::error::Error;

/// One `[[watch]]` array entry as written in TOML: the rule's own fields
/// plus the `name` that makes it mergeable across layers (the spec's example
/// schema puts `name` inside the array entry, not as the map key, so this is
/// re-keyed into a map at load time rather than at the schema level).
#[derive(Debug, Deserialize)]
struct WatchRuleEntry {
    name: String,
    #[serde(flatten)]
    patch: WatchRulePatch,
}

/// The raw shape of a `lspd.toml`-family file.
#[derive(Debug, Default, Deserialize)]
struct ConfigFile {
    #[serde(default)]
    lsp: HashMap<String, LspDefinitionPatch>,
    #[serde(default)]
    lsps: ActivationPatch,
    #[serde(default)]
    watch: Vec<WatchRuleEntry>,
}

/// One layer's contribution to the effective configuration, already re-keyed
/// (watch rules by `name`) but not yet merged with any other layer.
#[derive(Debug, Default, Clone)]
pub struct ConfigPatch {
    /// `[lsp.<name>]` tables.
    pub lsp: HashMap<String, LspDefinitionPatch>,
    /// The `[lsps]` table.
    pub lsps: ActivationPatch,
    /// `[[watch]]` entries, keyed by their `name`.
    pub watch: HashMap<String, WatchRulePatch>,
}

impl ConfigPatch {
    /// Folds `patch` (a higher-priority layer) on top of `self`.
    #[must_use]
    pub fn merge(self, patch: Self) -> Self {
        Self {
            lsp: super::merge::merge_maps_by_key(self.lsp, patch.lsp, LspDefinitionPatch::merge),
            lsps: self.lsps.merge(patch.lsps),
            watch: super::merge::merge_maps_by_key(self.watch, patch.watch, WatchRulePatch::merge),
        }
    }
}

/// Loads and parses a single configuration file. Returns `Ok(None)` if the
/// file simply does not exist (the normal, expected case for
/// `lspd.toml`/`lspd.local.toml`, and even for the user config on a
/// freshly-installed machine) — only a genuine I/O error (permissions, a
/// directory where a file was expected, ...) or invalid TOML is reported.
pub fn load(path: &Path) -> crate::Result<Option<ConfigPatch>> {
    let contents = match std::fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(Error::io(path, err)),
    };

    let file: ConfigFile = toml::from_str(&contents).map_err(|source| Error::InvalidToml {
        path: path.to_path_buf(),
        source: Box::new(source),
    })?;

    let mut watch = HashMap::with_capacity(file.watch.len());
    for entry in file.watch {
        if watch.insert(entry.name.clone(), entry.patch).is_some() {
            return Err(Error::DuplicateWatchRule {
                name: entry.name,
                path: path.to_path_buf(),
            });
        }
    }

    Ok(Some(ConfigPatch {
        lsp: file.lsp,
        lsps: file.lsps,
        watch,
    }))
}

/// `$XDG_CONFIG_HOME/lspd/config.toml`, with normal XDG fallback
/// (`~/.config/lspd/config.toml` when unset) via the `directories` crate.
#[must_use]
pub fn user_config_path() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "lspd").map(|dirs| dirs.config_dir().join("config.toml"))
}

/// `lspd.toml` at the project root — optional, may be committed.
#[must_use]
pub fn project_config_path(root: &Path) -> PathBuf {
    root.join("lspd.toml")
}

/// `lspd.local.toml` at the project root — optional, normally untracked.
/// See the recommended `.gitignore` entry documented alongside
/// `docs/configuration.md`.
#[must_use]
pub fn local_config_path(root: &Path) -> PathBuf {
    root.join("lspd.local.toml")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn a_missing_file_loads_as_an_empty_patch_rather_than_an_error() {
        let tmp = TempDir::new().unwrap();
        let result = load(&tmp.path().join("lspd.toml")).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn duplicate_watch_rule_names_in_one_file_are_a_load_error() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("lspd.toml");
        std::fs::write(
            &path,
            r#"
            [[watch]]
            name = "tests"
            command = ["true"]

            [[watch]]
            name = "tests"
            command = ["false"]
            "#,
        )
        .unwrap();

        let err = load(&path).unwrap_err();
        assert!(matches!(err, Error::DuplicateWatchRule { name, .. } if name == "tests"));
    }

    #[test]
    fn a_well_formed_file_parses_lsp_activation_and_watch_sections() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("lspd.toml");
        std::fs::write(
            &path,
            r#"
            [lsps]
            enabled = ["pyright", "ruff"]

            [lsp.pyright]
            command = ["pyright-langserver", "--stdio"]

            [[watch]]
            name = "frontend-tests"
            paths = ["frontend/**/*.ts"]
            command = ["mise", "run", "test:frontend"]
            "#,
        )
        .unwrap();

        let patch = load(&path).unwrap().unwrap();
        assert_eq!(
            patch.lsps.enabled,
            Some(vec!["pyright".to_string(), "ruff".to_string()])
        );
        assert!(patch.lsp.contains_key("pyright"));
        assert!(patch.watch.contains_key("frontend-tests"));
    }
}
