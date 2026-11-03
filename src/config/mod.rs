//! The configuration engine: the "first vertical slice" (spec §26) from
//! layered TOML files down to an effective, template-rendered configuration.
//!
//! ```text
//! built-in registry
//!     -> user config      ($XDG_CONFIG_HOME/lspd/config.toml)
//!     -> lspd.toml
//!     -> lspd.local.toml
//!     -> template/environment resolution
//!     -> effective configuration
//! ```
//!
//! Definitions ([`lsp::LspDefinitionPatch`], merged by key) and activation
//! ([`activation::ActivationPatch`], replaced wholesale) are folded together
//! into one [`layer::ConfigPatch`] per layer, then merged across layers —
//! but they stay independently resolvable afterwards: [`EffectiveConfig`]
//! resolves an LSP definition on demand, separately from computing the
//! enabled list, so a `lsp config <name>` query never has to first decide
//! whether `<name>` is active.

pub mod activation;
pub mod layer;
pub mod lsp;
pub mod merge;
pub mod registry;
pub mod watch;

use std::collections::HashMap;
use std::path::PathBuf;

use crate::error::Error;
use crate::mise::{self, EnvironmentProvider, MiseProvider};
use crate::project::ProjectRoot;
use crate::templating::TemplateContext;

use layer::ConfigPatch;
use lsp::{LspDefinitionPatch, ResolvedLspDefinition};
use watch::{WatchRule, WatchRulePatch};

/// The fully merged (but not yet per-definition resolved/rendered)
/// configuration for a project, plus everything needed to resolve
/// definitions and watch rules on demand.
pub struct EffectiveConfig {
    root: ProjectRoot,
    definitions: HashMap<String, LspDefinitionPatch>,
    watch: HashMap<String, WatchRulePatch>,
    enabled: Vec<String>,
    context: TemplateContext,
    /// Which configuration files actually existed and contributed a layer —
    /// exposed for `lspd status`/`project info` diagnostics, so a user
    /// can see *why* a given value won without re-deriving the layering by
    /// hand.
    sources: Vec<PathBuf>,
}

impl EffectiveConfig {
    /// Resolves the effective configuration for `root` using the Mise
    /// provider. The convenience entry point every command should use;
    /// [`Self::resolve_with`] exists so tests can substitute a stub
    /// environment provider.
    pub fn resolve(root: &ProjectRoot) -> crate::Result<Self> {
        Self::resolve_with(root, &MiseProvider)
    }

    /// Resolves the effective configuration using a specific
    /// [`EnvironmentProvider`] — the seam `docs/adr/0007` describes for a
    /// future Nix provider, and what lets unit tests avoid shelling out to
    /// a real `mise`.
    pub fn resolve_with(
        root: &ProjectRoot,
        provider: &impl EnvironmentProvider,
    ) -> crate::Result<Self> {
        let mut merged = ConfigPatch {
            lsp: registry::builtin().clone(),
            ..Default::default()
        };
        let mut sources = Vec::new();

        let mut layer_paths: Vec<Option<PathBuf>> = vec![layer::user_config_path()];
        layer_paths.push(Some(layer::project_config_path(root.path())));
        layer_paths.push(Some(layer::local_config_path(root.path())));

        for path in layer_paths.into_iter().flatten() {
            if let Some(patch) = layer::load(&path)? {
                sources.push(path);
                merged = merged.merge(patch);
            }
        }

        let detected_types = root.detect_types();
        let enabled = activation::resolve(merged.lsps.clone(), &merged.lsp, &detected_types);

        let environment = mise::effective_environment(provider, root.path());
        let context = TemplateContext::new(environment, root.path());

        Ok(Self {
            root: root.clone(),
            definitions: merged.lsp,
            watch: merged.watch,
            enabled,
            context,
            sources,
        })
    }

    /// The project this configuration was resolved for.
    #[must_use]
    pub fn root(&self) -> &ProjectRoot {
        &self.root
    }

    /// The configuration files that contributed a layer, in precedence order
    /// (lowest first). The built-in registry is never listed since it is not
    /// a file on disk.
    #[must_use]
    pub fn sources(&self) -> &[PathBuf] {
        &self.sources
    }

    /// Every LSP name known in this project — from the registry or any
    /// configuration layer — regardless of whether it is active. Sorted for
    /// deterministic `--json` output.
    #[must_use]
    pub fn lsp_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.definitions.keys().cloned().collect();
        names.sort();
        names
    }

    /// The resolved, effective set of active LSP names (spec §7).
    #[must_use]
    pub fn enabled(&self) -> &[String] {
        &self.enabled
    }

    /// Whether `name` is in the effective activation list.
    #[must_use]
    pub fn is_enabled(&self, name: &str) -> bool {
        self.enabled.iter().any(|n| n == name)
    }

    /// Resolves and renders one named LSP definition — the effective
    /// configuration a `lspd lsp config <name>` call returns (spec §18).
    /// Resolution is on demand and per-name: an unrelated definition with a
    /// bad template or a missing command never blocks this call.
    pub fn resolve_lsp(&self, name: &str) -> crate::Result<ResolvedLspDefinition> {
        let patch = self
            .definitions
            .get(name)
            .cloned()
            .ok_or_else(|| Error::UnknownLsp {
                name: name.to_string(),
            })?;

        let resolved = patch.resolve(name)?;
        self.render_lsp(resolved)
    }

    fn render_lsp(&self, def: ResolvedLspDefinition) -> crate::Result<ResolvedLspDefinition> {
        Ok(ResolvedLspDefinition {
            executable: self.context.render(&def.executable)?,
            args: def
                .args
                .into_iter()
                .map(|a| self.context.render(&a))
                .collect::<crate::Result<_>>()?,
            environment: self.context.render_map(def.environment)?,
            initialization_options: def
                .initialization_options
                .map(|v| self.context.render_json(v))
                .transpose()?,
            settings: def
                .settings
                .map(|v| self.context.render_json(v))
                .transpose()?,
            capabilities: def
                .capabilities
                .map(|v| self.context.render_json(v))
                .transpose()?,
            ..def
        })
    }

    /// Every resolvable watch rule, rendered. A rule with no command in any
    /// layer is silently omitted (see [`WatchRulePatch::resolve`]) rather
    /// than failing the whole configuration.
    pub fn watch_rules(&self) -> crate::Result<Vec<WatchRule>> {
        let mut rules: Vec<WatchRule> = self
            .watch
            .iter()
            .filter_map(|(name, patch)| patch.clone().resolve(name))
            .collect();
        rules.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(rules)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap as Map;
    use std::path::Path;
    use tempfile::TempDir;

    struct StubProvider;
    impl EnvironmentProvider for StubProvider {
        fn resolve(&self, _root: &Path) -> Map<String, String> {
            Map::new()
        }
    }

    fn python_project(extra_files: &[(&str, &str)]) -> TempDir {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("pyproject.toml"), "").unwrap();
        for (name, contents) in extra_files {
            std::fs::write(tmp.path().join(name), contents).unwrap();
        }
        tmp
    }

    #[test]
    fn a_zero_config_python_project_auto_enables_both_pyright_and_ruff() {
        let tmp = python_project(&[]);
        let root = ProjectRoot::discover(tmp.path()).unwrap();
        let config = EffectiveConfig::resolve_with(&root, &StubProvider).unwrap();

        assert_eq!(
            config.enabled(),
            &["pyright".to_string(), "ruff".to_string()]
        );
    }

    #[test]
    fn both_pyright_and_ruff_resolve_to_distinct_independently_configured_definitions() {
        let tmp = python_project(&[]);
        let root = ProjectRoot::discover(tmp.path()).unwrap();
        let config = EffectiveConfig::resolve_with(&root, &StubProvider).unwrap();

        let pyright = config.resolve_lsp("pyright").unwrap();
        let ruff = config.resolve_lsp("ruff").unwrap();

        assert_ne!(pyright.executable, ruff.executable);
        assert!(config.is_enabled("pyright"));
        assert!(config.is_enabled("ruff"));
    }

    #[test]
    fn project_config_can_override_one_definition_without_touching_activation() {
        let tmp = python_project(&[(
            "lspd.toml",
            r#"
            [lsp.pyright]
            command = ["basedpyright-langserver", "--stdio"]
            "#,
        )]);
        let root = ProjectRoot::discover(tmp.path()).unwrap();
        let config = EffectiveConfig::resolve_with(&root, &StubProvider).unwrap();

        let pyright = config.resolve_lsp("pyright").unwrap();
        assert_eq!(pyright.executable, "basedpyright-langserver");
        // Activation was never mentioned in lspd.toml, so zero-config
        // auto-enable still applies unchanged.
        assert_eq!(
            config.enabled(),
            &["pyright".to_string(), "ruff".to_string()]
        );
    }

    #[test]
    fn local_config_can_change_activation_without_redefining_any_lsp() {
        let tmp = python_project(&[(
            "lspd.local.toml",
            r#"
            [lsps]
            enabled = ["ruff"]
            "#,
        )]);
        let root = ProjectRoot::discover(tmp.path()).unwrap();
        let config = EffectiveConfig::resolve_with(&root, &StubProvider).unwrap();

        assert_eq!(config.enabled(), &["ruff".to_string()]);
        // pyright is still a perfectly resolvable definition, just inactive.
        assert!(config.resolve_lsp("pyright").is_ok());
        assert!(!config.is_enabled("pyright"));
    }

    #[test]
    fn local_config_overrides_project_config_for_the_same_field() {
        let tmp = python_project(&[
            (
                "lspd.toml",
                r#"
                [lsp.pyright.settings.python.analysis]
                typeCheckingMode = "basic"
                "#,
            ),
            (
                "lspd.local.toml",
                r#"
                [lsp.pyright.settings.python.analysis]
                typeCheckingMode = "strict"
                "#,
            ),
        ]);
        let root = ProjectRoot::discover(tmp.path()).unwrap();
        let config = EffectiveConfig::resolve_with(&root, &StubProvider).unwrap();

        let pyright = config.resolve_lsp("pyright").unwrap();
        assert_eq!(
            pyright.settings.unwrap()["python"]["analysis"]["typeCheckingMode"],
            "strict"
        );
    }

    #[test]
    fn resolving_an_unknown_lsp_name_fails_with_a_clear_error() {
        let tmp = python_project(&[]);
        let root = ProjectRoot::discover(tmp.path()).unwrap();
        let config = EffectiveConfig::resolve_with(&root, &StubProvider).unwrap();

        let err = config.resolve_lsp("does-not-exist").unwrap_err();
        assert!(matches!(err, Error::UnknownLsp { name } if name == "does-not-exist"));
    }

    #[test]
    fn settings_templates_render_against_the_effective_environment() {
        let tmp = python_project(&[(
            "lspd.toml",
            r#"
            [lsp.pyright.settings.python.analysis]
            extraPaths = ["{{ env.PROJECT_ROOT }}/src"]
            "#,
        )]);
        let root = ProjectRoot::discover(tmp.path()).unwrap();
        let config = EffectiveConfig::resolve_with(&root, &StubProvider).unwrap();

        let pyright = config.resolve_lsp("pyright").unwrap();
        let expected = format!("{}/src", root.path().display());
        assert_eq!(
            pyright.settings.unwrap()["python"]["analysis"]["extraPaths"][0],
            expected
        );
    }
}
