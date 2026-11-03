//! Integration tests for the MCP server (spec §22), served over an
//! in-memory duplex transport rather than real stdio — this exercises the
//! actual `rmcp` protocol (tool listing, JSON-RPC call framing, structured
//! output) without spawning a subprocess.

use std::fs;

use lspd::mcp::LspdServer;
use lspd::project::ProjectRoot;
use rmcp::ServiceExt;
use rmcp::model::CallToolRequestParams;
use serde_json::json;

fn python_project() -> tempfile::TempDir {
    let tmp = tempfile::TempDir::new().unwrap();
    fs::write(tmp.path().join("pyproject.toml"), "").unwrap();
    tmp
}

/// Spawns the server over one end of a duplex pipe and returns a client
/// (rmcp's blanket `()` `ClientHandler`) connected to the other end.
async fn connect(root: ProjectRoot) -> rmcp::service::RunningService<rmcp::RoleClient, ()> {
    let (server_transport, client_transport) = tokio::io::duplex(4096);

    tokio::spawn(async move {
        let _ = LspdServer::new(root)
            .serve(server_transport)
            .await
            .unwrap()
            .waiting()
            .await;
    });

    ().serve(client_transport)
        .await
        .expect("the client should complete MCP initialization")
}

#[tokio::test]
async fn the_tool_list_includes_every_documented_capability() {
    let tmp = python_project();
    let root = ProjectRoot::discover(tmp.path()).unwrap();
    let client = connect(root).await;

    let tools = client.list_tools(None).await.unwrap();
    let names: Vec<&str> = tools.tools.iter().map(|t| t.name.as_ref()).collect();

    for expected in [
        "project_info",
        "project_capabilities",
        "lsp_list",
        "lsp_config",
        "lsp_status",
        "lsp_ensure",
        "task_list",
        "task_run",
    ] {
        assert!(
            names.contains(&expected),
            "expected tool `{expected}` in {names:?}"
        );
    }

    // Spec §22: raw LSP traffic and the raw event stream stay off MCP.
    assert!(!names.contains(&"lsp_endpoint"));
    assert!(!names.contains(&"events"));

    client.cancel().await.unwrap();
}

#[tokio::test]
async fn project_info_reports_both_registry_python_lsps() {
    let tmp = python_project();
    let root = ProjectRoot::discover(tmp.path()).unwrap();
    let client = connect(root).await;

    let result = client
        .call_tool(CallToolRequestParams::new("project_info"))
        .await
        .unwrap();

    let structured = result
        .structured_content
        .expect("project_info returns structured JSON");
    let enabled = structured["enabled_lsps"].as_array().unwrap();
    let names: Vec<&str> = enabled.iter().map(|v| v.as_str().unwrap()).collect();
    assert!(names.contains(&"pyright"));
    assert!(names.contains(&"ruff"));

    client.cancel().await.unwrap();
}

#[tokio::test]
async fn lsp_config_returns_the_same_effective_configuration_the_cli_would() {
    let tmp = python_project();
    let root = ProjectRoot::discover(tmp.path()).unwrap();
    let client = connect(root).await;

    let args: serde_json::Map<String, serde_json::Value> =
        serde_json::from_value(json!({ "name": "pyright" })).unwrap();
    let result = client
        .call_tool(CallToolRequestParams::new("lsp_config").with_arguments(args))
        .await
        .unwrap();

    let structured = result
        .structured_content
        .expect("lsp_config returns structured JSON");
    assert_eq!(structured["executable"], "pyright-langserver");
    assert_eq!(structured["args"][0], "--stdio");

    client.cancel().await.unwrap();
}

#[tokio::test]
async fn lsp_config_for_an_unknown_name_reports_a_tool_error_not_a_protocol_error() {
    let tmp = python_project();
    let root = ProjectRoot::discover(tmp.path()).unwrap();
    let client = connect(root).await;

    let args: serde_json::Map<String, serde_json::Value> =
        serde_json::from_value(json!({ "name": "does-not-exist" })).unwrap();
    let result = client
        .call_tool(CallToolRequestParams::new("lsp_config").with_arguments(args))
        .await
        .unwrap();

    assert_eq!(result.is_error, Some(true));

    client.cancel().await.unwrap();
}
