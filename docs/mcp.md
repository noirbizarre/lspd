# MCP

`lspd mcp serve` runs a minimal [MCP](https://modelcontextprotocol.io)
server over stdio (via [`rmcp`](https://docs.rs/rmcp)), scoped to one project
for its whole lifetime.

MCP is complementary to native LSP, not a replacement for it: every tool
below is a higher-level lspd capability, and each calls the exact same
engine functions (`config::EffectiveConfig`, `client::DaemonClient`) that the
equivalent CLI subcommand calls — no protocol-specific logic is duplicated
between the two surfaces.

| Tool | Equivalent CLI command | Needs a daemon |
|---|---|---|
| `project_info` | `project info` | no |
| `project_capabilities` | `project capabilities` | no |
| `lsp_list` | `lsp list` | no |
| `lsp_config` | `lsp config <name>` | no |
| `lsp_status` | `lsp status <name>` | yes (starts one) |
| `lsp_ensure` | `lsp ensure <name>` | yes (starts one) |
| `task_list` | `task list` | no |
| `task_run` | `task run <name>` | no |

## Deliberately not exposed over MCP

- **Raw LSP traffic** (`lsp endpoint` / the proxy socket). An agent that
  needs to speak LSP directly uses a real LSP client against that socket —
  encoding the entire LSP protocol into MCP tool calls would duplicate a
  standard that already exists (spec §22).
- **The raw event stream** (`events`). It is a general-purpose primitive for
  editors and long-lived watchers, not a request/response capability; an
  agent that wants filesystem notifications connects to the control socket
  directly.

## Example

```console
$ lspd mcp serve
```

Then, from an MCP client, call `lsp_config` with `{"name": "pyright"}` to get
the same fully-rendered configuration `lspd lsp config pyright --json`
would print — templates already resolved, no client-side rendering required
(see [Configuration](configuration.md#templating)).
