//! The control-socket wire protocol: newline-delimited JSON, internally
//! tagged so a frame is self-describing without external framing metadata.
//!
//! Every field added after v1 must be `#[serde(default)]`, so an older
//! client's frames keep parsing against a newer daemon and vice versa —
//! the same backward-compatibility discipline used for `snypr`'s IPC layer,
//! justified here by the same reasoning: a daemon and a CLI invocation can
//! be built from different `projectd` versions on a developer's machine, and
//! a torn connection over an optional field nobody needed is a worse failure
//! mode than ignoring it.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::events::FsEvent;
use super::handshake::Handshake;
use super::lsp_manager::LspStatusInfo;

/// A request sent by a client on the control socket.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Request {
    /// Must be the first request on any connection. See
    /// `daemon::handshake`.
    Handshake,
    /// Report a named LSP instance's lifecycle status.
    LspStatus {
        /// The definition's name.
        name: String,
    },
    /// Start (or reuse) a named LSP instance.
    LspEnsure {
        /// The definition's name.
        name: String,
    },
    /// Ensure a named LSP instance, then return its proxy socket path.
    LspEndpoint {
        /// The definition's name.
        name: String,
    },
    /// Switches this connection into event-stream mode: the daemon pushes
    /// `Response::Event` frames until the client disconnects. No further
    /// requests are read on this connection afterwards.
    Subscribe {
        /// Only forward events touching a path matching one of these globs.
        /// An empty list means "everything".
        #[serde(default)]
        paths: Vec<String>,
    },
    /// List configured watch rules and their current (config default,
    /// possibly runtime-overridden) enabled state.
    WatchList,
    /// Enable or disable a watch rule for this daemon's remaining lifetime.
    /// Not persisted to any configuration file — see
    /// `daemon::watch_runtime`.
    WatchSet {
        /// The rule's name.
        name: String,
        /// The new state.
        enabled: bool,
    },
    /// Politely asks the daemon to shut down.
    Shutdown,
}

/// A response frame. `Error` is always a valid answer to any request — a
/// connection is never simply dropped on failure (spec-adjacent discipline
/// borrowed from `snypr`'s IPC layer: "a client always gets a structured
/// frame back").
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Response {
    /// Answers `Request::Handshake`.
    Handshake(Handshake),
    /// Answers `Request::LspStatus`.
    LspStatus(LspStatusInfo),
    /// Answers `Request::LspEnsure`.
    LspEnsured {
        /// The definition's name (repeated for convenience alongside
        /// `status.name`, which callers already destructure this way).
        name: String,
        /// The resulting status.
        status: LspStatusInfo,
    },
    /// Answers `Request::LspEndpoint`.
    LspEndpoint {
        /// The Unix socket path for raw LSP stdio proxying.
        socket: PathBuf,
    },
    /// One event pushed after `Request::Subscribe`. Repeated until the
    /// connection closes.
    Event(FsEvent),
    /// Answers `Request::WatchList`.
    ///
    /// A struct variant with one named field rather than a newtype variant
    /// wrapping a bare `Vec` — serde's internally-tagged representation
    /// (`#[serde(tag = "type")]`) can only merge the tag into a JSON object,
    /// and a bare sequence has no object to merge it into. This is covered
    /// by a wire-shape test in this module's `tests`, not just a round-trip
    /// one, precisely because round-tripping alone never catches this: a
    /// failure to *serialize* never gets far enough to round-trip anything.
    WatchList {
        /// Every configured rule's current runtime state.
        rules: Vec<WatchRuleInfo>,
    },
    /// Answers `Request::WatchSet` and `Request::Shutdown`.
    Ack,
    /// A structured failure — see [`Request`]'s module docs.
    Error {
        /// A stable, machine-readable diagnostic code, matching
        /// `error::Error`'s `projectd::<module>::<kind>` convention.
        code: String,
        /// A human-readable explanation.
        message: String,
    },
}

/// One watch rule's runtime state, as reported by `Request::WatchList`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchRuleInfo {
    /// The rule's name.
    pub name: String,
    /// Glob patterns it applies to.
    pub paths: Vec<String>,
    /// Whether it is currently active (configuration default, folded with
    /// any runtime `WatchSet` override).
    pub enabled: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    // Round-tripping alone can stay green even if a field silently stops
    // being written on one side, so these pin the literal wire shape too —
    // the lesson `snypr`'s IPC tests document explicitly.

    #[test]
    fn a_handshake_request_serializes_to_the_documented_wire_shape() {
        let json = serde_json::to_string(&Request::Handshake).unwrap();
        assert_eq!(json, r#"{"type":"handshake"}"#);
    }

    #[test]
    fn an_lsp_ensure_request_carries_its_name_field() {
        let json = serde_json::to_string(&Request::LspEnsure {
            name: "pyright".to_string(),
        })
        .unwrap();
        assert_eq!(json, r#"{"type":"lsp_ensure","name":"pyright"}"#);
    }

    #[test]
    fn a_subscribe_request_with_no_paths_omits_nothing_thanks_to_the_default() {
        let json = serde_json::to_string(&Request::Subscribe { paths: vec![] }).unwrap();
        assert_eq!(json, r#"{"type":"subscribe","paths":[]}"#);

        // And the reverse: a frame with the field missing entirely (as an
        // older client that predates `paths` would send) still parses.
        let legacy = r#"{"type":"subscribe"}"#;
        let parsed: Request = serde_json::from_str(legacy).unwrap();
        assert!(matches!(parsed, Request::Subscribe { paths } if paths.is_empty()));
    }

    #[test]
    fn every_request_variant_round_trips_through_json() {
        let requests = vec![
            Request::Handshake,
            Request::LspStatus {
                name: "ruff".into(),
            },
            Request::LspEnsure {
                name: "ruff".into(),
            },
            Request::LspEndpoint {
                name: "ruff".into(),
            },
            Request::Subscribe {
                paths: vec!["**/*.rs".into()],
            },
            Request::WatchList,
            Request::WatchSet {
                name: "tests".into(),
                enabled: false,
            },
            Request::Shutdown,
        ];
        for request in requests {
            let json = serde_json::to_string(&request).unwrap();
            let parsed: Request = serde_json::from_str(&json).unwrap();
            assert_eq!(format!("{parsed:?}"), format!("{request:?}"));
        }
    }

    /// Round-tripping alone would not have caught the bug this guards
    /// against: a newtype variant wrapping a bare `Vec` fails to
    /// *serialize* under `#[serde(tag = "type")]` (serde cannot merge an
    /// internal tag into a JSON array), so it never gets far enough to
    /// round-trip anything. Every response variant is serialized here for
    /// exactly that reason — this must never regress to `unwrap()`-panicking
    /// mid-request the way `Response::WatchList` once did.
    #[test]
    fn every_response_variant_serializes_successfully() {
        let responses = vec![
            Response::Handshake(Handshake::new(std::path::PathBuf::from("/work/demo"))),
            Response::LspStatus(crate::daemon::lsp_manager::LspStatusInfo {
                name: "pyright".into(),
                status: crate::daemon::lsp_manager::LspStatus::Running,
                pid: Some(123),
            }),
            Response::LspEnsured {
                name: "pyright".into(),
                status: crate::daemon::lsp_manager::LspStatusInfo {
                    name: "pyright".into(),
                    status: crate::daemon::lsp_manager::LspStatus::Running,
                    pid: Some(123),
                },
            },
            Response::LspEndpoint {
                socket: PathBuf::from("/tmp/x.sock"),
            },
            Response::Event(crate::daemon::events::FsEvent::Created {
                path: PathBuf::from("src/main.rs"),
            }),
            Response::WatchList {
                rules: vec![WatchRuleInfo {
                    name: "tests".into(),
                    paths: vec!["**/*.rs".into()],
                    enabled: true,
                }],
            },
            Response::Ack,
            Response::Error {
                code: "projectd::lsp::busy".into(),
                message: "busy".into(),
            },
        ];
        for response in responses {
            serde_json::to_string(&response)
                .unwrap_or_else(|err| panic!("{response:?} must serialize: {err}"));
        }
    }

    #[test]
    fn an_error_response_carries_a_stable_code_and_a_message() {
        let response = Response::Error {
            code: "projectd::lsp::busy".to_string(),
            message: "LSP `pyright` already has an attached client".to_string(),
        };
        let json = serde_json::to_string(&response).unwrap();
        let parsed: Response = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, Response::Error { code, .. } if code == "projectd::lsp::busy"));
    }
}
