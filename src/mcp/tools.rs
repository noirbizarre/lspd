//! MCP tool argument and result schemas.
//!
//! Kept separate from `mcp::mod` (where the `#[tool_router]` impl and the
//! tool bodies live — the macro needs `#[tool]` methods directly in its own
//! impl block) purely for readability: these types are the wire shapes an
//! MCP client sees, worth reading independently of the dispatch logic.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::project::ProjectType;

/// Arguments shared by every tool that names one LSP definition.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct LspNameArgs {
    /// The LSP definition's stable name, e.g. `"pyright"`.
    pub name: String,
}

/// Arguments for `task_run`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct TaskRunArgs {
    /// The Mise task's name.
    pub name: String,
    /// Extra arguments forwarded to the task.
    #[serde(default)]
    pub args: Vec<String>,
}

/// Result of `project_info`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ProjectInfo {
    pub root: String,
    pub name: String,
    pub detected_types: Vec<ProjectType>,
    pub enabled_lsps: Vec<String>,
    pub known_lsps: Vec<String>,
}

/// Result of `project_capabilities`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct Capabilities {
    pub lsp: bool,
    pub events: bool,
    pub watch: bool,
    pub mcp: bool,
    pub task: bool,
}

/// One entry of `lsp_list`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct LspSummary {
    pub name: String,
    pub enabled: bool,
    pub languages: Vec<String>,
}

/// Result of `task_run`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct TaskRunResult {
    pub success: bool,
    pub exit_code: Option<i32>,
}
