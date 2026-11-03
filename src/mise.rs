//! The Mise environment provider (spec §13).
//!
//! Mise is a provider/implementation detail, not part of the LSPD
//! protocol: `EnvironmentProvider` is the seam a future Nix provider would
//! implement (`docs/adr/0007-mise-as-pluggable-environment-provider.md`).
//! LSPD never reimplements tool installation or version management —
//! it only consumes whatever environment `mise env` reports.

use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

use tracing::warn;

/// A source of the "effective environment" for a project — the variables an
/// LSP process should be launched with and that templates see under `env.*`.
pub trait EnvironmentProvider {
    /// Resolves the effective environment for `root`. Implementations should
    /// prefer returning a best-effort environment (e.g. falling back to
    /// `std::env::vars()`) over failing outright: a missing/broken provider
    /// must not make LSPD unusable in a project that does not need it.
    fn resolve(&self, root: &Path) -> HashMap<String, String>;
}

/// Shells out to `mise env --json`, merging its output over the process's own
/// environment (Mise wins on overlap — that is the entire point of running
/// it). If the `mise` binary is missing or exits non-zero, logs a warning and
/// falls back to the plain OS environment rather than failing: not every
/// project uses Mise, and zero-config behaviour must not depend on it being
/// installed.
#[derive(Debug, Default, Clone, Copy)]
pub struct MiseProvider;

impl EnvironmentProvider for MiseProvider {
    fn resolve(&self, root: &Path) -> HashMap<String, String> {
        let mut env: HashMap<String, String> = std::env::vars().collect();

        match run_mise_env(root) {
            Ok(mise_env) => env.extend(mise_env),
            Err(message) => {
                warn!(root = %root.display(), %message, "mise env failed, falling back to the OS environment");
            }
        }

        env
    }
}

/// Injects the one variable LSPD itself owns: `PROJECT_ROOT`. Applied
/// after any provider runs, so neither Mise nor the OS environment can
/// shadow it — templates must be able to rely on it unconditionally
/// (spec §6's example, `{{ env.PROJECT_ROOT }}/src`).
pub fn effective_environment(
    provider: &impl EnvironmentProvider,
    root: &Path,
) -> HashMap<String, String> {
    let mut env = provider.resolve(root);
    env.insert("PROJECT_ROOT".to_string(), root.display().to_string());
    env
}

fn run_mise_env(root: &Path) -> Result<HashMap<String, String>, String> {
    let output = Command::new("mise")
        .arg("env")
        .arg("--json")
        .current_dir(root)
        .output()
        .map_err(|err| format!("could not spawn `mise`: {err}"))?;

    if !output.status.success() {
        return Err(format!(
            "`mise env --json` exited with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    serde_json::from_slice::<HashMap<String, String>>(&output.stdout)
        .map_err(|err| format!("could not parse `mise env --json` output: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    struct StubProvider(HashMap<String, String>);
    impl EnvironmentProvider for StubProvider {
        fn resolve(&self, _root: &Path) -> HashMap<String, String> {
            self.0.clone()
        }
    }

    #[test]
    fn project_root_is_always_injected_and_cannot_be_shadowed_by_the_provider() {
        let tmp = TempDir::new().unwrap();
        let stub = StubProvider(HashMap::from([(
            "PROJECT_ROOT".to_string(),
            "/somewhere/else".to_string(),
        )]));

        let env = effective_environment(&stub, tmp.path());

        assert_eq!(
            env.get("PROJECT_ROOT"),
            Some(&tmp.path().display().to_string())
        );
    }

    #[test]
    fn provider_variables_are_preserved_alongside_the_injected_root() {
        let tmp = TempDir::new().unwrap();
        let stub = StubProvider(HashMap::from([(
            "PYTHONPATH".to_string(),
            "/venv".to_string(),
        )]));

        let env = effective_environment(&stub, tmp.path());

        assert_eq!(env.get("PYTHONPATH"), Some(&"/venv".to_string()));
    }
}
