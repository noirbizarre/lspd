//! `projectd mcp serve` — a thin wrapper: the actual MCP server (tool
//! definitions, `ServerHandler` wiring) lives in `projectd::mcp`, since it is
//! itself a protocol server like the daemon, not a formatting concern of
//! this binary crate.

use super::Context;

pub async fn serve(ctx: &Context) -> projectd::Result<()> {
    projectd::mcp::serve(ctx.root.clone()).await
}
