//! Marker-file → project-type detection.
//!
//! Used only to drive zero-config LSP activation (`config::activation`): a
//! project with no `lspd.toml` at all should still get sensible LSPs
//! enabled, and the only signal available for that is "what kind of project
//! is this", inferred from well-known files.

use std::path::Path;

/// A coarse language/ecosystem classification of a project root.
///
/// This is deliberately not the same vocabulary as LSP `languages`/filetypes
/// (which are per-file, e.g. `"python"` vs `"pyi"`): it is one classification
/// per marker file found, used only to decide which registry entries to
/// auto-enable.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum ProjectType {
    /// `Cargo.toml` present.
    Rust,
    /// `pyproject.toml`, `setup.py`, or `setup.cfg` present.
    Python,
    /// `go.mod` present.
    Go,
    /// `package.json` present.
    JavaScript,
    /// `package.json` *and* `tsconfig.json` present. Reported in addition to
    /// [`ProjectType::JavaScript`], not instead of it — a TypeScript project
    /// is also a JavaScript one for registry-matching purposes.
    TypeScript,
}

impl ProjectType {
    /// The registry `languages` tag this project type auto-enables
    /// definitions for.
    #[must_use]
    pub fn language(self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::Python => "python",
            Self::Go => "go",
            Self::JavaScript => "javascript",
            Self::TypeScript => "typescript",
        }
    }
}

/// One (marker file, resulting type) pair. A `Vec` rather than a `HashMap`
/// since detection order does not matter but determinism of iteration
/// (mostly for tests and JSON output) does — this evaluates in a fixed order.
const MARKERS: &[(&str, ProjectType)] = &[
    ("Cargo.toml", ProjectType::Rust),
    ("pyproject.toml", ProjectType::Python),
    ("setup.py", ProjectType::Python),
    ("setup.cfg", ProjectType::Python),
    ("go.mod", ProjectType::Go),
    ("package.json", ProjectType::JavaScript),
];

/// Detects every project type whose marker file exists directly at `root`.
///
/// Only looks at `root` itself, not subdirectories: a monorepo with a
/// `frontend/package.json` and a `backend/Cargo.toml` is intentionally out of
/// scope for zero-config detection in the MVP (an explicit `lspd.toml` is
/// the documented answer for monorepos).
pub fn detect(root: &Path) -> Vec<ProjectType> {
    let mut found: Vec<ProjectType> = MARKERS
        .iter()
        .filter(|(marker, _)| root.join(marker).exists())
        .map(|(_, ty)| *ty)
        .collect();

    // TypeScript is additive to JavaScript, keyed off a second marker file
    // rather than its own entry in `MARKERS`, so `package.json` alone never
    // implies a `tsconfig.json`-shaped project.
    if found.contains(&ProjectType::JavaScript) && root.join("tsconfig.json").exists() {
        found.push(ProjectType::TypeScript);
    }

    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn a_directory_with_no_marker_files_detects_no_project_types() {
        let tmp = TempDir::new().unwrap();
        assert_eq!(detect(tmp.path()), Vec::new());
    }

    #[test]
    fn pyproject_toml_detects_as_python() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("pyproject.toml"), "").unwrap();
        assert_eq!(detect(tmp.path()), vec![ProjectType::Python]);
    }

    #[test]
    fn package_json_alone_detects_as_javascript_only() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("package.json"), "{}").unwrap();
        assert_eq!(detect(tmp.path()), vec![ProjectType::JavaScript]);
    }

    #[test]
    fn package_json_with_tsconfig_detects_as_javascript_and_typescript() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("package.json"), "{}").unwrap();
        fs::write(tmp.path().join("tsconfig.json"), "{}").unwrap();
        assert_eq!(
            detect(tmp.path()),
            vec![ProjectType::JavaScript, ProjectType::TypeScript]
        );
    }

    #[test]
    fn multiple_unrelated_markers_all_detect() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("Cargo.toml"), "").unwrap();
        fs::write(tmp.path().join("go.mod"), "").unwrap();
        assert_eq!(detect(tmp.path()), vec![ProjectType::Rust, ProjectType::Go]);
    }
}
