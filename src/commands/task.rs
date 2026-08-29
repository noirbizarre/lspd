//! `projectd task <...>` — thin delegation to Mise (spec §13: "Do not
//! reimplement these systems").
//!
//! Daemon-independent: task execution has no lifecycle of its own worth a
//! daemon managing, unlike an LSP process or a persistent watcher.

use projectd::Error;

use super::Context;

pub async fn list(ctx: &Context) -> projectd::Result<()> {
    let mut command = tokio::process::Command::new("mise");
    command.arg("tasks").arg("ls").current_dir(ctx.root.path());
    if ctx.json {
        command.arg("--json");
    }
    run_inherited(command).await
}

pub async fn run(ctx: &Context, name: &str, args: Vec<String>) -> projectd::Result<()> {
    let mut command = tokio::process::Command::new("mise");
    command.arg("run").arg(name).current_dir(ctx.root.path());
    if !args.is_empty() {
        command.arg("--").args(args);
    }
    run_inherited(command).await
}

/// Runs `command` with inherited stdio, so a human sees Mise's own output
/// directly rather than ProjectD re-wrapping it. `--json` is forwarded to
/// Mise itself for `task list` rather than ProjectD attempting to
/// re-envelope Mise's own JSON shape, which is Mise's contract to keep
/// stable, not ProjectD's to duplicate.
async fn run_inherited(mut command: tokio::process::Command) -> projectd::Result<()> {
    let status = command
        .status()
        .await
        .map_err(|source| Error::io("mise", source))?;

    if !status.success() {
        return Err(Error::io(
            "mise",
            std::io::Error::other(format!("mise exited with {status}")),
        ));
    }
    Ok(())
}
