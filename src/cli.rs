//! Argument types only.
//!
//! No behaviour lives here: parsing is one concern and doing the work is
//! another, and keeping them apart is what lets the library be used without
//! the CLI. See `src/commands/*` for the actual implementations.

use std::path::PathBuf;

use clap::{Parser, Subcommand};

/// Project-scoped development infrastructure: shared LSPs, filesystem events and MCP for editors and agents
#[derive(Debug, Parser)]
#[command(name = "lspd", version, about, long_about = None)]
pub struct Cli {
    /// Increase verbosity. Repeat for more (`-v` info, `-vv` debug, `-vvv` trace).
    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    pub verbose: u8,

    /// Emit machine-readable JSON on stdout instead of human text.
    #[arg(long, global = true)]
    pub json: bool,

    /// The project root to operate on. Defaults to walking up from the
    /// current directory looking for `.git` or `lspd.toml`.
    #[arg(long, global = true, value_name = "PATH")]
    pub project: Option<PathBuf>,

    /// The subcommand to run.
    #[command(subcommand)]
    pub command: Command,
}

/// The subcommands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Show whether a daemon is running for this project, and a summary of
    /// its effective configuration.
    Status,

    /// Project inspection.
    #[command(subcommand)]
    Project(ProjectCommand),

    /// LSP definitions, activation and lifecycle.
    #[command(subcommand)]
    Lsp(LspCommand),

    /// Subscribe to the shared filesystem event stream.
    Events {
        /// Only print events whose path matches this glob. May be repeated.
        #[arg(long = "path", value_name = "GLOB")]
        paths: Vec<String>,
    },

    /// Watch rules — optional event-stream consumers that run commands.
    #[command(subcommand)]
    Watch(WatchCommand),

    /// Mise task delegation.
    #[command(subcommand)]
    Task(TaskCommand),

    /// Daemon plumbing. Not normally invoked directly — other commands start
    /// a daemon on demand (spec §14's discover-or-start lifecycle).
    Daemon {
        /// Run detached from the terminal, as `client::discover_or_start`
        /// does internally. Without this flag, runs in the foreground with
        /// logs on stderr — the form to use while debugging the daemon
        /// itself.
        #[arg(long)]
        detach: bool,
    },

    /// Run a minimal MCP server over stdio for AI agents (spec §22).
    Mcp,
}

/// `lspd project <...>`.
#[derive(Debug, Subcommand)]
pub enum ProjectCommand {
    /// The canonical root, detected project types, and configuration sources.
    Info,
    /// What this LSPD instance can do for the project (LSPs, events,
    /// watch rules, MCP tools) — a machine-discoverable capability summary.
    Capabilities,
}

/// `lspd lsp <...>`.
#[derive(Debug, Subcommand)]
pub enum LspCommand {
    /// List every known LSP definition and whether it is active.
    List,
    /// Print one definition's fully resolved, template-rendered configuration.
    Config {
        /// The definition's stable name.
        name: String,
    },
    /// Report a running instance's lifecycle status. Requires a daemon.
    Status {
        /// The definition's stable name.
        name: String,
    },
    /// Start (or reuse) a running instance. Requires a daemon.
    Ensure {
        /// The definition's stable name.
        name: String,
    },
    /// Print the Unix socket a client can attach to for raw LSP stdio
    /// proxying. Requires a daemon; ensures the instance first.
    Endpoint {
        /// The definition's stable name.
        name: String,
    },
}

/// `lspd watch <...>`.
#[derive(Debug, Subcommand)]
pub enum WatchCommand {
    /// List configured watch rules and their enabled state.
    List,
    /// Enable a watch rule for the running daemon's lifetime.
    Enable {
        /// The rule's name.
        name: String,
    },
    /// Disable a watch rule for the running daemon's lifetime.
    Disable {
        /// The rule's name.
        name: String,
    },
}

/// `lspd task <...>`.
#[derive(Debug, Subcommand)]
pub enum TaskCommand {
    /// List Mise tasks available in this project.
    List,
    /// Run a Mise task, in the effective Mise environment.
    Run {
        /// The task's name.
        name: String,
        /// Extra arguments forwarded to the task.
        #[arg(trailing_var_arg = true)]
        args: Vec<String>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    // Catches the derive mistakes that would otherwise only surface as a panic
    // the first time a user runs the binary.
    #[test]
    fn the_command_definition_is_valid() {
        Cli::command().debug_assert();
    }
}
