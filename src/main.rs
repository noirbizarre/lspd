//! The lspd binary.
//!
//! Thin by construction: `cli.rs` parses, `commands/*` execute (calling into
//! the `lspd` library for everything that is not printing or argument
//! handling), and this file only wires the two together plus process-level
//! concerns (tracing, exit codes).

#![allow(clippy::result_large_err)]

use std::process::ExitCode;

use clap::Parser;

mod cli;
mod commands;
mod output;

use cli::{Cli, Command, LspCommand, ProjectCommand, TaskCommand, WatchCommand};
use commands::Context;

fn main() -> ExitCode {
    let args = Cli::parse();
    init_tracing(args.verbose);

    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("failed to start the async runtime: {error}");
            return ExitCode::FAILURE;
        }
    };

    runtime.block_on(run(args))
}

async fn run(args: Cli) -> ExitCode {
    let ctx = match Context::new(args.project, args.json) {
        Ok(ctx) => ctx,
        Err(error) => {
            report_startup_error(args.json, error);
            return ExitCode::FAILURE;
        }
    };

    let result = dispatch(&ctx, args.command).await;

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            commands::report_error(&ctx, error);
            ExitCode::FAILURE
        }
    }
}

async fn dispatch(ctx: &Context, command: Command) -> lspd::Result<()> {
    match command {
        Command::Status => commands::status::run(ctx).await,
        Command::Project(ProjectCommand::Info) => commands::project::info(ctx),
        Command::Project(ProjectCommand::Capabilities) => {
            commands::project::capabilities(ctx);
            Ok(())
        }
        Command::Lsp(LspCommand::List) => commands::lsp::list(ctx),
        Command::Lsp(LspCommand::Config { name }) => commands::lsp::config(ctx, &name),
        Command::Lsp(LspCommand::Status { name }) => commands::lsp::status(ctx, &name).await,
        Command::Lsp(LspCommand::Ensure { name }) => commands::lsp::ensure(ctx, &name).await,
        Command::Lsp(LspCommand::Endpoint { name }) => commands::lsp::endpoint(ctx, &name).await,
        Command::Events { paths } => commands::events::run(ctx, paths).await,
        Command::Watch(WatchCommand::List) => commands::watch::list(ctx).await,
        Command::Watch(WatchCommand::Enable { name }) => commands::watch::enable(ctx, &name).await,
        Command::Watch(WatchCommand::Disable { name }) => {
            commands::watch::disable(ctx, &name).await
        }
        Command::Task(TaskCommand::List) => commands::task::list(ctx).await,
        Command::Task(TaskCommand::Run { name, args }) => {
            commands::task::run(ctx, &name, args).await
        }
        Command::Daemon { detach: _ } => commands::daemon::serve(ctx).await,
        Command::Mcp => commands::mcp::serve(ctx).await,
    }
}

/// A `Context::new` failure (project root resolution) happens before we have
/// a `Context` to route through `commands::report_error`, so this handles
/// the `--json`/human split directly for that one early-exit case.
fn report_startup_error(json: bool, error: lspd::Error) {
    if json {
        let code = miette::Diagnostic::code(&error)
            .map(|c| c.to_string())
            .unwrap_or_else(|| "lspd::error::unknown".to_string());
        crate::output::print_err(&code, &error.to_string());
    } else {
        eprintln!("{:?}", miette::Report::new(error));
    }
}

/// Wires `-v`/`-vv`/`-vvv` (and `RUST_LOG`, which always wins if set) to
/// `tracing`. A daemon running detached has no terminal to color for, so
/// this deliberately does not special-case TTY detection the way `output`
/// does for `--json`.
fn init_tracing(verbosity: u8) {
    use tracing_subscriber::EnvFilter;

    let default_level = match verbosity {
        0 => "warn",
        1 => "info",
        2 => "debug",
        _ => "trace",
    };

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(format!("lspd={default_level}")));

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .init();
}
