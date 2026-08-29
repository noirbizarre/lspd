//! A minimal MCP server for AI agents (spec §22).
//!
//! MCP is complementary to native LSP, not a reimplementation of it: every
//! tool here is a higher-level ProjectD capability (project info, effective
//! configuration, service lifecycle, task execution) that calls the exact
//! same engine functions the CLI's `commands::*` call — `config` for
//! configuration queries, `client` for anything stateful. Raw LSP traffic
//! and the raw event stream stay off MCP entirely: an agent that needs
//! those uses the control socket or a real LSP client, per spec §22's
//! "do not encode the entire LSP protocol into MCP".

mod tools;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{ServerCapabilities, ServerInfo};
use rmcp::{Json, handler::server::router::tool::ToolRouter};
use rmcp::{ServerHandler, ServiceExt, tool, tool_handler, tool_router};

use crate::client;
use crate::config::EffectiveConfig;
use crate::daemon::lsp_manager::LspStatusInfo;
use crate::daemon::protocol::{Request, Response};
use crate::project::ProjectRoot;

use tools::{Capabilities, LspNameArgs, LspSummary, ProjectInfo, TaskRunArgs, TaskRunResult};

/// The MCP server, scoped to one project for its whole lifetime — an agent
/// wanting a different project starts a different `projectd mcp serve`
/// (matching how the CLI resolves `--project`/cwd once per invocation).
#[derive(Clone)]
pub struct ProjectdServer {
    root: ProjectRoot,
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl ProjectdServer {
    /// Builds a server scoped to `root`. Public so integration tests can
    /// serve it over an in-memory transport rather than only via
    /// [`serve`]'s real stdio.
    #[must_use]
    pub fn new(root: ProjectRoot) -> Self {
        Self {
            root,
            tool_router: Self::tool_router(),
        }
    }

    /// The canonical root, detected project types, and known/enabled LSPs.
    #[tool(
        description = "Report the project's root, detected types, and known/enabled LSP definitions"
    )]
    async fn project_info(&self) -> Result<Json<ProjectInfo>, String> {
        let config = resolve_config(&self.root)?;
        Ok(Json(ProjectInfo {
            root: self.root.path().display().to_string(),
            name: self.root.name(),
            detected_types: self.root.detect_types(),
            enabled_lsps: config.enabled().to_vec(),
            known_lsps: config.lsp_names(),
        }))
    }

    /// What this ProjectD instance can do for the project.
    #[tool(
        description = "Report which ProjectD capabilities are available (lsp, events, watch, task)"
    )]
    fn project_capabilities(&self) -> Json<Capabilities> {
        Json(Capabilities {
            lsp: true,
            events: true,
            watch: true,
            mcp: true,
            task: true,
        })
    }

    /// Every known LSP definition and whether it is active.
    #[tool(
        description = "List every LSP definition known in this project and whether it is active"
    )]
    async fn lsp_list(&self) -> Result<Json<Vec<LspSummary>>, String> {
        let config = resolve_config(&self.root)?;
        let summaries = config
            .lsp_names()
            .into_iter()
            .filter_map(|name| {
                let resolved = config.resolve_lsp(&name).ok()?;
                Some(LspSummary {
                    enabled: config.is_enabled(&name),
                    languages: resolved.languages,
                    name,
                })
            })
            .collect();
        Ok(Json(summaries))
    }

    /// One definition's fully resolved, template-rendered configuration.
    #[tool(
        description = "Return one LSP definition's fully resolved, template-rendered configuration"
    )]
    async fn lsp_config(
        &self,
        Parameters(args): Parameters<LspNameArgs>,
    ) -> Result<Json<crate::config::lsp::ResolvedLspDefinition>, String> {
        let config = resolve_config(&self.root)?;
        config
            .resolve_lsp(&args.name)
            .map(Json)
            .map_err(|e| e.to_string())
    }

    /// A running instance's lifecycle status. Starts a daemon if needed.
    #[tool(
        description = "Report a named LSP instance's lifecycle status, starting a daemon if needed"
    )]
    async fn lsp_status(
        &self,
        Parameters(args): Parameters<LspNameArgs>,
    ) -> Result<Json<LspStatusInfo>, String> {
        let mut conn = client::discover_or_start(&self.root)
            .await
            .map_err(|e| e.to_string())?;
        match client::request(&mut conn, Request::LspStatus { name: args.name })
            .await
            .map_err(|e| e.to_string())?
        {
            Response::LspStatus(status) => Ok(Json(status)),
            other => Err(format!("unexpected response: {other:?}")),
        }
    }

    /// Starts (or reuses) a named LSP instance.
    #[tool(description = "Start or reuse a named LSP instance")]
    async fn lsp_ensure(
        &self,
        Parameters(args): Parameters<LspNameArgs>,
    ) -> Result<Json<LspStatusInfo>, String> {
        let mut conn = client::discover_or_start(&self.root)
            .await
            .map_err(|e| e.to_string())?;
        match client::request(&mut conn, Request::LspEnsure { name: args.name })
            .await
            .map_err(|e| e.to_string())?
        {
            Response::LspEnsured { status, .. } => Ok(Json(status)),
            other => Err(format!("unexpected response: {other:?}")),
        }
    }

    /// Mise tasks available in this project.
    #[tool(description = "List Mise tasks available in this project")]
    async fn task_list(&self) -> Result<Json<serde_json::Value>, String> {
        let output = tokio::process::Command::new("mise")
            .arg("tasks")
            .arg("ls")
            .arg("--json")
            .current_dir(self.root.path())
            .output()
            .await
            .map_err(|e| e.to_string())?;
        serde_json::from_slice(&output.stdout)
            .map(Json)
            .map_err(|e| e.to_string())
    }

    /// Runs a Mise task in the effective Mise environment.
    #[tool(description = "Run a Mise task in this project's effective environment")]
    async fn task_run(
        &self,
        Parameters(args): Parameters<TaskRunArgs>,
    ) -> Result<Json<TaskRunResult>, String> {
        let mut command = tokio::process::Command::new("mise");
        command
            .arg("run")
            .arg(&args.name)
            .current_dir(self.root.path());
        if !args.args.is_empty() {
            command.arg("--").args(&args.args);
        }
        let status = command.status().await.map_err(|e| e.to_string())?;
        Ok(Json(TaskRunResult {
            success: status.success(),
            exit_code: status.code(),
        }))
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for ProjectdServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
    }
}

fn resolve_config(root: &ProjectRoot) -> Result<EffectiveConfig, String> {
    EffectiveConfig::resolve(root).map_err(|e| e.to_string())
}

/// Runs the MCP server over stdio until the peer disconnects.
pub async fn serve(root: ProjectRoot) -> crate::Result<()> {
    let server = ProjectdServer::new(root)
        .serve(rmcp::transport::stdio())
        .await
        .map_err(|source| crate::Error::io("<mcp stdio>", std::io::Error::other(source)))?;
    server
        .waiting()
        .await
        .map_err(|source| crate::Error::io("<mcp stdio>", std::io::Error::other(source)))?;
    Ok(())
}
