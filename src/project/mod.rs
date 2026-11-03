//! Project/worktree root detection.
//!
//! LSPD's identity for a running daemon is a canonical filesystem path,
//! not whatever relative directory a client happened to invoke it from — two
//! clients in `src/` and `tests/` of the same checkout must resolve to the
//! same instance. Detection therefore always walks upward.

pub mod detect;

use std::path::{Path, PathBuf};

pub use detect::ProjectType;

/// Marker files/directories that, when found while walking upward from a
/// starting directory, are taken as "this is the project root". `.git` is
/// checked first and wins over any of the language markers: a monorepo with
/// a `Cargo.toml` in a subdirectory is still one project rooted at the
/// repository root, not one per crate.
const ROOT_MARKERS: &[&str] = &[".git", "lspd.toml"];

/// A resolved, canonicalized project root.
///
/// Canonicalization matters here specifically because it is hashed to derive
/// the daemon's socket path (`daemon::socket`): `.` and the absolute path it
/// resolves to must produce the same socket, or two clients in the same
/// project would each believe they own a different, absent daemon.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectRoot(PathBuf);

impl ProjectRoot {
    /// Walks upward from `start` looking for a root marker (`.git` or
    /// `lspd.toml`). Falls back to
    /// `start` itself (canonicalized) if none is found — LSPD must work
    /// with zero configuration and no VCS, per the spec, so "no marker
    /// found" is not an error, just a smaller-scoped root.
    pub fn discover(start: &Path) -> crate::Result<Self> {
        let start = start
            .canonicalize()
            .map_err(|source| crate::Error::io(start, source))?;

        let mut current = start.as_path();
        loop {
            if ROOT_MARKERS
                .iter()
                .any(|marker| current.join(marker).exists())
            {
                return Ok(Self(current.to_path_buf()));
            }
            match current.parent() {
                Some(parent) => current = parent,
                // Reached the filesystem root without finding a marker:
                // the starting directory itself is the best available root.
                None => return Ok(Self(start)),
            }
        }
    }

    /// The canonical, absolute path of the project root.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.0
    }

    /// A short, human-facing name for the project — the root directory's
    /// file name, or the full path if the root has none (e.g. `/`).
    #[must_use]
    pub fn name(&self) -> String {
        self.0
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.0.display().to_string())
    }

    /// Detects which project types apply at this root, by presence of marker
    /// files. Used to drive zero-config LSP activation (`config::activation`).
    #[must_use]
    pub fn detect_types(&self) -> Vec<ProjectType> {
        detect::detect(&self.0)
    }
}

impl std::fmt::Display for ProjectRoot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.display())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn a_directory_with_no_markers_anywhere_above_it_is_its_own_root() {
        let tmp = TempDir::new().unwrap();
        let nested = tmp.path().join("a/b/c");
        fs::create_dir_all(&nested).unwrap();

        let root = ProjectRoot::discover(&nested).unwrap();

        assert_eq!(root.path(), nested.canonicalize().unwrap());
    }

    #[test]
    fn a_git_marker_in_an_ancestor_directory_is_found() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join(".git")).unwrap();
        let nested = tmp.path().join("src/deeply/nested");
        fs::create_dir_all(&nested).unwrap();

        let root = ProjectRoot::discover(&nested).unwrap();

        assert_eq!(root.path(), tmp.path().canonicalize().unwrap());
    }

    #[test]
    fn a_closer_marker_wins_over_a_farther_one() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join(".git")).unwrap();
        let inner = tmp.path().join("vendor/subproject");
        fs::create_dir_all(&inner).unwrap();
        fs::write(inner.join("lspd.toml"), "").unwrap();
        let nested = inner.join("src");
        fs::create_dir_all(&nested).unwrap();

        let root = ProjectRoot::discover(&nested).unwrap();

        assert_eq!(root.path(), inner.canonicalize().unwrap());
    }
}
