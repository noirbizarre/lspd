//! `projectd lsp <...>`.
//!
//! `list` and `config` are daemon-independent (pure configuration
//! resolution); `status`, `ensure` and `endpoint` need a live daemon since
//! they concern a running process, not just its configuration, and go
//! through `crate::client::DaemonClient`.

use serde::Serialize;

use projectd::config::EffectiveConfig;
use projectd::daemon::protocol::{Request, Response};

use super::Context;
use projectd::client;

#[derive(Debug, Serialize)]
struct LspSummary {
    name: String,
    enabled: bool,
    languages: Vec<String>,
    resolvable: bool,
    error: Option<String>,
}

pub fn list(ctx: &Context) -> projectd::Result<()> {
    let config = EffectiveConfig::resolve(&ctx.root)?;

    let summaries: Vec<LspSummary> = config
        .lsp_names()
        .into_iter()
        .map(|name| match config.resolve_lsp(&name) {
            Ok(resolved) => LspSummary {
                enabled: config.is_enabled(&name),
                languages: resolved.languages.clone(),
                resolvable: true,
                error: None,
                name,
            },
            Err(err) => LspSummary {
                enabled: config.is_enabled(&name),
                languages: Vec::new(),
                resolvable: false,
                error: Some(err.to_string()),
                name,
            },
        })
        .collect();

    if ctx.json {
        crate::output::print_ok(&summaries);
    } else if summaries.is_empty() {
        println!("(no LSP definitions known for this project)");
    } else {
        for s in &summaries {
            let marker = if s.enabled { "*" } else { " " };
            let status = if s.resolvable {
                s.languages.join(",")
            } else {
                format!("error: {}", s.error.as_deref().unwrap_or("unknown"))
            };
            println!("{marker} {:<20} {status}", s.name);
        }
    }

    Ok(())
}

pub fn config(ctx: &Context, name: &str) -> projectd::Result<()> {
    let config = EffectiveConfig::resolve(&ctx.root)?;
    let resolved = config.resolve_lsp(name)?;

    if ctx.json {
        crate::output::print_ok(&resolved);
    } else {
        println!("name:    {}", resolved.name);
        println!("command: {}", resolved.command_line().join(" "));
        println!("languages: {}", resolved.languages.join(", "));
        if !resolved.environment.is_empty() {
            println!("environment:");
            let mut keys: Vec<_> = resolved.environment.keys().collect();
            keys.sort();
            for key in keys {
                println!("  {key}={}", resolved.environment[key]);
            }
        }
        if let Some(settings) = &resolved.settings {
            println!(
                "settings: {}",
                serde_json::to_string_pretty(settings).unwrap_or_default()
            );
        }
        if let Some(opts) = &resolved.initialization_options {
            println!(
                "initializationOptions: {}",
                serde_json::to_string_pretty(opts).unwrap_or_default()
            );
        }
    }

    Ok(())
}

pub async fn status(ctx: &Context, name: &str) -> projectd::Result<()> {
    let mut conn = client::discover_or_start(&ctx.root).await?;
    let response = client::request(
        &mut conn,
        Request::LspStatus {
            name: name.to_string(),
        },
    )
    .await?;
    match response {
        Response::LspStatus(info) => {
            if ctx.json {
                crate::output::print_ok(&info);
            } else {
                println!("{}: {:?}", name, info.status);
            }
            Ok(())
        }
        other => Err(client::unexpected_response(other)),
    }
}

pub async fn ensure(ctx: &Context, name: &str) -> projectd::Result<()> {
    let mut conn = client::discover_or_start(&ctx.root).await?;
    let response = client::request(
        &mut conn,
        Request::LspEnsure {
            name: name.to_string(),
        },
    )
    .await?;
    match response {
        Response::LspEnsured { name, status } => {
            if ctx.json {
                crate::output::print_ok(&status);
            } else {
                println!("{name}: {:?}", status.status);
            }
            Ok(())
        }
        other => Err(client::unexpected_response(other)),
    }
}

pub async fn endpoint(ctx: &Context, name: &str) -> projectd::Result<()> {
    let mut conn = client::discover_or_start(&ctx.root).await?;
    let response = client::request(
        &mut conn,
        Request::LspEndpoint {
            name: name.to_string(),
        },
    )
    .await?;
    match response {
        Response::LspEndpoint { socket } => {
            if ctx.json {
                crate::output::print_ok(&serde_json::json!({ "socket": socket }));
            } else {
                println!("{}", socket.display());
            }
            Ok(())
        }
        other => Err(client::unexpected_response(other)),
    }
}
