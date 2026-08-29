//! Command implementations.
//!
//! `cli.rs` parses; this module executes. Kept in the binary crate rather
//! than the library, deliberately: these functions print (human text or the
//! `output` envelope) and depend on `cli`'s argument shapes, and "nothing in
//! the library knows a command exists" (`AGENTS.md`) means that side of the
//! seam belongs here, not in `projectd::*`. Everything they actually *do* —
//! resolving configuration, talking to a daemon — is a plain library call,
//! which is also what `crate::mcp` calls to expose the same capabilities to
//! agents without duplicating this printing layer.

pub mod daemon;
pub mod events;
pub mod lsp;
pub mod mcp;
pub mod project;
pub mod status;
pub mod task;
pub mod watch;

use std::path::PathBuf;

use projectd::project::ProjectRoot;

/// Shared context every command runs with: the resolved project root and
/// the global `--json`/`--verbose` flags.
pub struct Context {
    pub root: ProjectRoot,
    pub json: bool,
}

impl Context {
    /// Resolves the project root (from `--project` if given, else by
    /// walking up from the current directory) and builds a `Context`.
    pub fn new(project: Option<PathBuf>, json: bool) -> projectd::Result<Self> {
        let start = match project {
            Some(path) => path,
            None => std::env::current_dir().map_err(|source| projectd::Error::io(".", source))?,
        };
        let root = ProjectRoot::discover(&start)?;
        Ok(Self { root, json })
    }
}

/// Reports a library [`projectd::Error`] the way `--json` vs human mode
/// expects. Every command's `run` should route its `Result::Err` arm through
/// this rather than letting `main` render it generically, so a `--json`
/// caller gets a structured error frame instead of a miette report on
/// stderr mixed into an otherwise-JSON stdout stream.
pub fn report_error(ctx: &Context, error: projectd::Error) {
    if ctx.json {
        let code = miette::Diagnostic::code(&error)
            .map(|c| c.to_string())
            .unwrap_or_else(|| "projectd::error::unknown".to_string());
        projectd::output::print_err(&code, &error.to_string());
    } else {
        eprintln!("{:?}", miette::Report::new(error));
    }
}
