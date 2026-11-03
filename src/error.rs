//! The error type, and the diagnostics it renders to.
//!
//! `thiserror` defines them, `miette` renders them. A diagnostic must carry the
//! two things the user does not already know: what specifically failed, and
//! what to do about it.

use std::path::PathBuf;

use miette::Diagnostic;
use thiserror::Error;

/// The crate's result type.
pub type Result<T> = std::result::Result<T, Error>;

/// Everything that can go wrong.
///
/// Diagnostic codes are `lspd::<module>::<kind>`. A code is a public
/// identifier users grep for, so renaming one is a breaking change.
#[derive(Debug, Error, Diagnostic)]
#[non_exhaustive]
pub enum Error {
    /// Reading or writing a file failed.
    #[error("failed to access `{path}`")]
    #[diagnostic(code(lspd::error::io))]
    Io {
        /// The path that could not be accessed.
        path: String,
        /// Why.
        #[source]
        source: std::io::Error,
    },

    /// A configuration layer parsed as invalid TOML.
    #[error("`{path}` is not valid TOML")]
    #[diagnostic(
        code(lspd::config::invalid_toml),
        help("check the syntax against the example in lspd's configuration docs")
    )]
    InvalidToml {
        /// The file that failed to parse.
        path: PathBuf,
        /// The underlying parse error.
        #[source]
        source: Box<toml::de::Error>,
    },

    /// An LSP activation or `lsp config`/`lsp ensure` request named a
    /// definition that does not exist in any configuration layer or the
    /// built-in registry.
    #[error("no LSP definition named `{name}`")]
    #[diagnostic(
        code(lspd::config::unknown_lsp),
        help("run `lspd lsp list` to see the definitions known in this project")
    )]
    UnknownLsp {
        /// The requested, unresolvable name.
        name: String,
    },

    /// An LSP definition resolved with no `command` set anywhere across the
    /// registry and the three configuration layers.
    #[error("LSP definition `{name}` has no command")]
    #[diagnostic(
        code(lspd::config::missing_command),
        help("add `command = [...]` to `[lsp.{name}]` in a configuration layer")
    )]
    MissingCommand {
        /// The definition missing a command.
        name: String,
    },

    /// A `[[watch]]` rule name appeared twice within a single configuration
    /// file. Names are how rules merge across layers, so duplicates within
    /// one layer cannot be resolved.
    #[error("watch rule `{name}` is defined twice in `{path}`")]
    #[diagnostic(code(lspd::config::duplicate_watch_rule))]
    DuplicateWatchRule {
        /// The rule name repeated.
        name: String,
        /// The file it was repeated in.
        path: PathBuf,
    },

    /// Rendering a MiniJinja template embedded in the configuration failed.
    #[error("failed to render template `{template}`")]
    #[diagnostic(
        code(lspd::template::render),
        help(
            "check the `{{ }}` expression against the fields exposed under `env.*` and `project.*`"
        )
    )]
    TemplateRender {
        /// The offending template source (or a short excerpt of it).
        template: String,
        /// Why MiniJinja rejected it.
        #[source]
        source: Box<minijinja::Error>,
    },

    /// Shelling out to `mise` to resolve the effective environment failed in
    /// a way that a plain `std::env::vars()` fallback cannot recover from
    /// (the binary ran but exited non-zero, or its JSON was unparsable).
    #[error("`mise env` failed for `{root}`")]
    #[diagnostic(
        code(lspd::mise::env_failed),
        help("run `mise env --json` in the project directory to see the underlying error")
    )]
    MiseEnv {
        /// The project root `mise env` was run against.
        root: PathBuf,
        /// Details.
        message: String,
    },

    /// No project root could be resolved from the given starting directory.
    #[error("`{path}` does not exist")]
    #[diagnostic(
        code(lspd::project::not_found),
        help("pass an existing directory to `--project`, or run lspd from inside one")
    )]
    ProjectPathNotFound {
        /// The path that was checked.
        path: PathBuf,
    },

    /// A control-socket connection completed a handshake that named a
    /// different protocol, or a protocol version this build does not speak.
    #[error("incompatible lspd protocol: expected `{expected}`, got `{actual}`")]
    #[diagnostic(
        code(lspd::daemon::protocol_mismatch),
        help("the client and daemon were built from different lspd versions; restart the daemon")
    )]
    ProtocolMismatch {
        /// What this build speaks.
        expected: String,
        /// What the daemon offered.
        actual: String,
    },

    /// A daemon could not be reached and could not be started.
    #[error("no lspd daemon is running for this project, and none could be started")]
    #[diagnostic(
        code(lspd::daemon::unreachable),
        help("run `lspd daemon serve` in the foreground to see why it fails to start")
    )]
    DaemonUnreachable {
        /// The underlying connection error.
        #[source]
        source: std::io::Error,
    },

    /// A request was sent to a daemon that returned a structured error frame.
    #[error("{message}")]
    #[diagnostic(code(lspd::daemon::request_failed))]
    RequestFailed {
        /// The stable machine-readable code the daemon reported.
        code: String,
        /// The human-readable explanation.
        message: String,
    },

    /// A second client tried to attach to an LSP proxy socket that already
    /// has an exclusive attached client (see
    /// `docs/adr/0005-single-exclusive-lsp-proxy-client.md`).
    #[error("LSP `{name}` already has an attached client")]
    #[diagnostic(
        code(lspd::lsp::busy),
        help("only one client may proxy a given LSP instance at a time in this MVP")
    )]
    LspBusy {
        /// The busy instance's name.
        name: String,
    },

    /// Serialising or deserialising a JSON wire message failed.
    #[error("malformed JSON message")]
    #[diagnostic(code(lspd::daemon::malformed_json))]
    MalformedJson {
        /// The underlying serde error.
        #[source]
        source: serde_json::Error,
    },
}

impl Error {
    /// Wraps an I/O error with the path that caused it, since
    /// `std::io::Error` alone rarely says which of several paths failed.
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into().display().to_string(),
            source,
        }
    }
}
