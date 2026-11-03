//! `lspd watch <...>`. All three require a daemon: watch rules only mean
//! anything against the daemon's persistent watcher, not a one-shot CLI
//! invocation.

use lspd::client;
use lspd::daemon::protocol::{Request, Response};

use super::Context;

pub async fn list(ctx: &Context) -> lspd::Result<()> {
    let mut conn = client::discover_or_start(&ctx.root).await?;
    let response = client::request(&mut conn, Request::WatchList).await?;
    match response {
        Response::WatchList { rules } => {
            if ctx.json {
                crate::output::print_ok(&rules);
            } else if rules.is_empty() {
                println!("(no watch rules configured)");
            } else {
                for rule in rules {
                    let marker = if rule.enabled { "*" } else { " " };
                    println!("{marker} {:<20} {}", rule.name, rule.paths.join(", "));
                }
            }
            Ok(())
        }
        other => Err(client::unexpected_response(other)),
    }
}

async fn set(ctx: &Context, name: &str, enabled: bool) -> lspd::Result<()> {
    let mut conn = client::discover_or_start(&ctx.root).await?;
    let response = client::request(
        &mut conn,
        Request::WatchSet {
            name: name.to_string(),
            enabled,
        },
    )
    .await?;
    match response {
        Response::Ack => {
            if ctx.json {
                crate::output::print_ok(&serde_json::json!({ "name": name, "enabled": enabled }));
            } else {
                println!("{name}: {}", if enabled { "enabled" } else { "disabled" });
            }
            Ok(())
        }
        other => Err(client::unexpected_response(other)),
    }
}

pub async fn enable(ctx: &Context, name: &str) -> lspd::Result<()> {
    set(ctx, name, true).await
}

pub async fn disable(ctx: &Context, name: &str) -> lspd::Result<()> {
    set(ctx, name, false).await
}
