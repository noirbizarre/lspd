//! Discovery: "where is the lspd instance for this project?" (spec §16),
//! answered as a pure function of the canonical project root rather than a
//! lookup table (`docs/adr/0004-deterministic-socket-discovery.md`) — two
//! clients resolving the same root always compute the same socket path
//! without needing to ask anything first.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

/// The directory every project's sockets and proxy-socket subdirectories
/// live under: `$XDG_RUNTIME_DIR/lspd`, falling back to
/// `/tmp/lspd-<uid>` when `XDG_RUNTIME_DIR` is unset (e.g. a non-systemd
/// system, or a login without a session manager).
#[must_use]
pub fn runtime_dir() -> PathBuf {
    match std::env::var_os("XDG_RUNTIME_DIR") {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir).join("lspd"),
        _ => std::env::temp_dir().join(format!("lspd-{}", current_uid())),
    }
}

fn current_uid() -> u32 {
    // SAFETY: `getuid` has no preconditions and never fails.
    unsafe { libc::getuid() }
}

/// Derives a deterministic, filesystem-safe identifier from a canonical
/// project root: the same root always hashes to the same value, on the same
/// machine or a different one, which is what lets discovery be a pure
/// function rather than a registry lookup.
#[must_use]
pub fn project_hash(root: &Path) -> String {
    let mut hasher = Sha256::new();
    hasher.update(root.as_os_str().as_encoded_bytes());
    let digest = hasher.finalize();
    // 16 hex chars (64 bits) is ample to avoid collisions among the number
    // of projects one machine ever has open, and keeps the socket path
    // comfortably under the ~104 byte `sockaddr_un` limit on most Unixes.
    digest.iter().take(8).map(|b| format!("{b:02x}")).collect()
}

/// The control socket path for `root`.
#[must_use]
pub fn control_socket_path(root: &Path) -> PathBuf {
    runtime_dir().join(format!("{}.sock", project_hash(root)))
}

/// The directory holding per-LSP-instance proxy sockets for `root`, kept
/// separate from the control socket itself so `lsp_manager` can create and
/// remove entries in it independently.
#[must_use]
pub fn proxy_sockets_dir(root: &Path) -> PathBuf {
    runtime_dir().join(project_hash(root))
}

/// Removes a stale socket file left behind by a crashed daemon, identified
/// by a failed connection attempt. Never removes a socket a live daemon is
/// still listening on — callers only reach this after `connect` already
/// failed with `ConnectionRefused`/`NotFound`.
pub fn remove_stale(path: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_root_always_hashes_to_the_same_value() {
        let root = Path::new("/work/demo");
        assert_eq!(project_hash(root), project_hash(root));
    }

    #[test]
    fn different_roots_hash_to_different_values() {
        assert_ne!(
            project_hash(Path::new("/work/demo")),
            project_hash(Path::new("/work/other"))
        );
    }

    #[test]
    fn the_control_socket_path_is_a_pure_function_of_the_root() {
        let root = Path::new("/work/demo");
        assert_eq!(control_socket_path(root), control_socket_path(root));
    }
}
