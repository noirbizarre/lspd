//! The handshake: "which LSPD instance is this, which protocol does it
//! speak, and what can it do" (spec §16) — deliberately a separate concept
//! from discovery (`daemon::socket`, "where is it").

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// The wire protocol's name. Checked on every handshake so a client talking
/// to the wrong kind of socket (or a stale one reused by another program)
/// fails clearly instead of misparsing frames.
pub const PROTOCOL_NAME: &str = "lspd";

/// The control protocol's version. Bump this whenever `protocol::Request`/
/// `protocol::Response` change in a way an older client could not safely
/// ignore (adding a new, optional, `#[serde(default)]` field does not
/// require a bump; changing or removing one does).
// Version 2 changes the required binary-version field to `lspd_version`.
pub const PROTOCOL_VERSION: u32 = 2;

/// Sent as the response to `Request::Handshake` — always the first exchange
/// on a freshly accepted connection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Handshake {
    /// Always [`PROTOCOL_NAME`]. Carried on the wire (rather than just
    /// assumed) so a client can give a clear error instead of a parse
    /// failure if it ever connects to the wrong socket.
    pub protocol: String,
    /// See [`PROTOCOL_VERSION`].
    pub protocol_version: u32,
    /// This daemon binary's `CARGO_PKG_VERSION`, for diagnostics.
    pub lspd_version: String,
    /// The canonical project root this daemon instance owns.
    pub project_root: PathBuf,
    /// Identifies this particular running process. Changes across restarts,
    /// even at the same socket path — lets a client notice "the daemon
    /// behind this socket is not the one I talked to a minute ago".
    pub instance_id: Uuid,
    /// Coarse feature flags, so a client can detect an older daemon that
    /// predates a given capability without needing a full version compare.
    pub capabilities: Vec<String>,
}

impl Handshake {
    /// Builds a fresh handshake for a daemon instance that just started.
    #[must_use]
    pub fn new(project_root: PathBuf) -> Self {
        Self {
            protocol: PROTOCOL_NAME.to_string(),
            protocol_version: PROTOCOL_VERSION,
            lspd_version: env!("CARGO_PKG_VERSION").to_string(),
            project_root,
            instance_id: Uuid::new_v4(),
            capabilities: vec!["lsp".to_string(), "events".to_string(), "watch".to_string()],
        }
    }

    /// Checks a handshake received from a peer against what this build
    /// speaks.
    pub fn verify(&self) -> crate::Result<()> {
        if self.protocol != PROTOCOL_NAME || self.protocol_version != PROTOCOL_VERSION {
            return Err(crate::Error::ProtocolMismatch {
                expected: format!("{PROTOCOL_NAME}/{PROTOCOL_VERSION}"),
                actual: format!("{}/{}", self.protocol, self.protocol_version),
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_freshly_built_handshake_verifies_against_itself() {
        let handshake = Handshake::new(PathBuf::from("/work/demo"));
        assert!(handshake.verify().is_ok());
    }

    #[test]
    fn the_handshake_serializes_the_documented_protocol_identity_and_binary_version() {
        let handshake = Handshake::new(PathBuf::from("/work/demo"));
        let value = serde_json::to_value(&handshake).unwrap();
        assert_eq!(value["protocol"], "lspd");
        assert_eq!(value["protocol_version"], 2);
        assert_eq!(value["lspd_version"], env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn a_mismatched_protocol_name_fails_verification() {
        let mut handshake = Handshake::new(PathBuf::from("/work/demo"));
        handshake.protocol = "another-daemon".to_string();
        let err = handshake.verify().unwrap_err();
        assert!(matches!(err, crate::Error::ProtocolMismatch { .. }));
    }

    #[test]
    fn a_mismatched_protocol_version_fails_verification() {
        let mut handshake = Handshake::new(PathBuf::from("/work/demo"));
        handshake.protocol_version += 1;
        let err = handshake.verify().unwrap_err();
        assert!(matches!(err, crate::Error::ProtocolMismatch { .. }));
    }
}
