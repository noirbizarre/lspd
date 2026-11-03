//! `lspd project info` / `lspd project capabilities`.
//!
//! Both are daemon-independent: they call `lspd::config::EffectiveConfig`
//! directly, in-process. A client asking "what is this project" should not
//! have to wait for (or start) a daemon just to find out.

use serde::Serialize;

use lspd::config::EffectiveConfig;

use super::Context;

#[derive(Debug, Serialize)]
struct ProjectInfo {
    root: String,
    name: String,
    detected_types: Vec<lspd::project::ProjectType>,
    configuration_sources: Vec<String>,
    enabled_lsps: Vec<String>,
    known_lsps: Vec<String>,
}

pub fn info(ctx: &Context) -> lspd::Result<()> {
    let config = EffectiveConfig::resolve(&ctx.root)?;

    let info = ProjectInfo {
        root: ctx.root.path().display().to_string(),
        name: ctx.root.name(),
        detected_types: ctx.root.detect_types(),
        configuration_sources: config
            .sources()
            .iter()
            .map(|p| p.display().to_string())
            .collect(),
        enabled_lsps: config.enabled().to_vec(),
        known_lsps: config.lsp_names(),
    };

    if ctx.json {
        crate::output::print_ok(&info);
    } else {
        println!("root:  {}", info.root);
        println!("name:  {}", info.name);
        println!(
            "types: {}",
            if info.detected_types.is_empty() {
                "(none detected)".to_string()
            } else {
                info.detected_types
                    .iter()
                    .map(|t| format!("{t:?}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            }
        );
        println!(
            "config sources: {}",
            if info.configuration_sources.is_empty() {
                "(none — zero-config)".to_string()
            } else {
                info.configuration_sources.join(", ")
            }
        );
        println!("known LSPs:   {}", info.known_lsps.join(", "));
        println!("enabled LSPs: {}", info.enabled_lsps.join(", "));
    }

    Ok(())
}

#[derive(Debug, Serialize)]
struct Capabilities {
    lsp: bool,
    events: bool,
    watch: bool,
    mcp: bool,
    task: bool,
}

pub fn capabilities(ctx: &Context) {
    // Static for the MVP: every daemon build offers the same capability set.
    // A future provider (Nix) or an LSP-less deployment could make some of
    // these conditional; the shape already carries per-capability booleans
    // rather than a single version number so that can happen without a
    // breaking change to callers that only check the flags they use.
    let capabilities = Capabilities {
        lsp: true,
        events: true,
        watch: true,
        mcp: true,
        task: true,
    };

    if ctx.json {
        crate::output::print_ok(&capabilities);
    } else {
        println!("lsp:    {}", capabilities.lsp);
        println!("events: {}", capabilities.events);
        println!("watch:  {}", capabilities.watch);
        println!("mcp:    {}", capabilities.mcp);
        println!("task:   {}", capabilities.task);
    }
}
