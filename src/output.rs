//! Machine-readable CLI output (spec §23).
//!
//! Lives in the binary crate, not the library: printing is a command-level
//! concern (`AGENTS.md`'s "nothing in the library knows a command exists"
//! extends to "the library never prints", since a caller using
//! `projectd::config`/`projectd::daemon` as a library — as `mcp::tools` does
//! — must never have output silently land on its stdout).
//!
//! Every command's `--json` payload is wrapped in one small, versioned
//! envelope, so a consumer (an agent, a script) can tell success from
//! failure and detect a breaking shape change without parsing the human
//! text. Pretty-printed on a TTY, compact otherwise — a human reading it
//! directly gets something readable, a pipeline gets one line.

use std::io::IsTerminal;

use serde::Serialize;

/// The current envelope schema version. Bump this, and document why in
/// `docs/protocol.md`, if the envelope's own shape ever changes in a way a
/// consumer could not infer from `payload` alone.
pub const ENVELOPE_VERSION: u32 = 1;

/// The stable top-level JSON shape every `--json` command emits on stdout.
#[derive(Debug, Serialize)]
struct Envelope<T: Serialize> {
    version: u32,
    status: Status,
    #[serde(skip_serializing_if = "Option::is_none")]
    payload: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<ErrorPayload>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum Status {
    Ok,
    Error,
}

#[derive(Debug, Serialize)]
struct ErrorPayload {
    /// The diagnostic code, e.g. `projectd::config::unknown_lsp` — the
    /// stable, grep-able identifier (see `AGENTS.md`'s error-handling rule).
    code: String,
    message: String,
}

/// Prints a successful `--json` payload to stdout.
pub fn print_ok<T: Serialize>(payload: &T) {
    print_envelope(&Envelope {
        version: ENVELOPE_VERSION,
        status: Status::Ok,
        payload: Some(payload),
        error: None,
    });
}

/// Prints a failed `--json` payload to stdout — used when `--json` is set
/// and a command fails, so a caller parsing stdout never has to fall back to
/// scraping stderr text to learn *why*.
pub fn print_err(code: &str, message: &str) {
    print_envelope(&Envelope::<()> {
        version: ENVELOPE_VERSION,
        status: Status::Error,
        payload: None,
        error: Some(ErrorPayload {
            code: code.to_string(),
            message: message.to_string(),
        }),
    });
}

fn print_envelope<T: Serialize>(envelope: &Envelope<T>) {
    let rendered = if std::io::stdout().is_terminal() {
        serde_json::to_string_pretty(envelope)
    } else {
        serde_json::to_string(envelope)
    }
    .expect("an envelope of already-serializable payloads must itself serialize");
    println!("{rendered}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_envelope_version_constant_matches_what_is_documented() {
        // A sentinel, not a real test of behaviour: if this ever needs to
        // change, `docs/protocol.md` must be updated in the same commit.
        assert_eq!(ENVELOPE_VERSION, 1);
    }
}
