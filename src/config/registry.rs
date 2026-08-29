//! The built-in LSP registry (spec §8).
//!
//! Compiled into the binary as a real TOML asset (`registry/builtin.toml`,
//! next to this file) rather than as Rust literals, so it parses through the
//! exact same [`super::lsp::LspDefinitionPatch`] deserialization as every
//! other configuration layer — one code path, one set of tests, and the
//! asset itself stays readable/extensible without touching Rust.

use std::collections::HashMap;
use std::sync::OnceLock;

use serde::Deserialize;

use super::lsp::LspDefinitionPatch;

/// The TOML asset, embedded at compile time.
const BUILTIN_TOML: &str = include_str!("registry/builtin.toml");

#[derive(Deserialize)]
struct RegistryFile {
    #[serde(default)]
    lsp: HashMap<String, LspDefinitionPatch>,
}

/// Returns the built-in registry's LSP definitions, parsed once and cached
/// for the process's lifetime — the asset is compiled in and never changes
/// at runtime, so re-parsing it per call would only cost time for nothing.
///
/// # Panics
///
/// Panics if `registry/builtin.toml` fails to parse. That file ships with the
/// binary and is covered by a unit test below; a parse failure here can only
/// mean a broken release, which should fail loudly rather than silently
/// running with an empty registry.
#[must_use]
pub fn builtin() -> &'static HashMap<String, LspDefinitionPatch> {
    static REGISTRY: OnceLock<HashMap<String, LspDefinitionPatch>> = OnceLock::new();
    REGISTRY.get_or_init(|| {
        toml::from_str::<RegistryFile>(BUILTIN_TOML)
            .expect("registry/builtin.toml must always parse")
            .lsp
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_builtin_registry_parses_without_error() {
        assert!(!builtin().is_empty());
    }

    #[test]
    fn python_has_two_independently_defined_auto_enable_lsps() {
        let registry = builtin();
        let pyright = registry
            .get("pyright")
            .expect("pyright should be registered");
        let ruff = registry.get("ruff").expect("ruff should be registered");

        assert_eq!(pyright.auto_enable, Some(true));
        assert_eq!(ruff.auto_enable, Some(true));
        assert_eq!(
            pyright.languages.as_deref(),
            Some(["python".to_string()].as_slice())
        );
        assert_eq!(
            ruff.languages.as_deref(),
            Some(["python".to_string()].as_slice())
        );
        // Distinct commands prove these are two independently configurable
        // definitions covering the same language, not one entry duplicated —
        // exactly the architecture the spec requires (§3, §26).
        assert_ne!(pyright.command, ruff.command);
    }

    #[test]
    fn every_builtin_entry_resolves_to_a_valid_definition() {
        for (name, patch) in builtin() {
            patch.clone().resolve(name).unwrap_or_else(|err| {
                panic!("builtin registry entry `{name}` must resolve: {err}")
            });
        }
    }
}
