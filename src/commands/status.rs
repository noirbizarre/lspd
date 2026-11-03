//! `lspd status`.
//!
//! Reports both the effective configuration summary (always available) and,
//! if a daemon happens to be reachable, its handshake — but never starts one
//! just to answer this: `status` must be a safe, side-effect-free thing to
//! run repeatedly (e.g. from a shell prompt).

use serde::Serialize;

use lspd::client;
use lspd::config::EffectiveConfig;

use super::Context;

#[derive(Debug, Serialize)]
struct StatusReport {
    root: String,
    enabled_lsps: Vec<String>,
    daemon: Option<DaemonStatus>,
}

#[derive(Debug, Serialize)]
struct DaemonStatus {
    instance_id: String,
    lspd_version: String,
    protocol_version: u32,
}

pub async fn run(ctx: &Context) -> lspd::Result<()> {
    let config = EffectiveConfig::resolve(&ctx.root)?;
    let daemon = client::try_handshake(&ctx.root)
        .await
        .map(|h| DaemonStatus {
            instance_id: h.instance_id.to_string(),
            lspd_version: h.lspd_version,
            protocol_version: h.protocol_version,
        });

    let report = StatusReport {
        root: ctx.root.path().display().to_string(),
        enabled_lsps: config.enabled().to_vec(),
        daemon,
    };

    if ctx.json {
        crate::output::print_ok(&report);
    } else {
        println!("root: {}", report.root);
        println!("enabled LSPs: {}", report.enabled_lsps.join(", "));
        match &report.daemon {
            Some(d) => println!(
                "daemon: running (lspd {}, protocol v{})",
                d.lspd_version, d.protocol_version
            ),
            None => println!("daemon: not running"),
        }
    }

    Ok(())
}
