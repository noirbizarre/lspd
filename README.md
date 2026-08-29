<p align="center">
  <img src="docs/images/icon.svg" alt="projectd" width="160">
</p>

<h1 align="center">projectd</h1>

<p align="center"><strong>Project-scoped development infrastructure: shared LSPs, filesystem events and MCP for editors and agents</strong></p>

<p align="center">
  <a href="https://github.com/noirbizarre/projectd/actions/workflows/ci.yaml">
    <img src="https://github.com/noirbizarre/projectd/actions/workflows/ci.yaml/badge.svg" alt="CI">
  </a>
  <a href="https://codecov.io/gh/noirbizarre/projectd">
    <img src="https://codecov.io/gh/noirbizarre/projectd/graph/badge.svg" alt="Codecov">
  </a>
  <img src="https://img.shields.io/github/v/release/noirbizarre/projectd" alt="Release">
  <a href="https://noirbizarre.github.io/projectd/">
    <img src="https://img.shields.io/badge/docs-noirbizarre.github.io-blue" alt="Documentation">
  </a>
  <img src="https://img.shields.io/github/license/noirbizarre/projectd" alt="License">
</p>

---

Editors, terminal tools, and AI coding agents each independently reimplement
the same project-scoped infrastructure — a language server per language, a
filesystem watcher, a task runner integration. projectd centralizes that:
one daemon per project, spawned on demand, sharing LSP servers, a filesystem
event stream, and Mise-provided environments across every client that asks
for them, over a Unix socket and a minimal [MCP](https://modelcontextprotocol.io)
server for agents. It composes existing standards (LSP stays LSP, Mise stays
the tool/environment provider) rather than replacing them.

## Installation

Download a binary for your platform from the
[latest release](https://github.com/noirbizarre/projectd/releases/latest).

## Usage

```bash
projectd project info              # detected project types, known/enabled LSPs
projectd lsp list                  # every LSP definition and its activation state
projectd lsp config pyright        # the fully resolved, template-rendered config
projectd lsp ensure pyright        # start (or reuse) a running instance
projectd lsp endpoint pyright      # the Unix socket for raw LSP stdio proxying
projectd events --path '**/*.py'   # subscribe to the shared filesystem event stream
projectd watch list                # configured watch rules and their state
projectd task run test             # delegates to `mise run test`
projectd mcp serve                 # a minimal MCP server for AI agents
```

Every command accepts `--json` for machine-readable output and `--project
<path>` to target a project other than the current directory. See the
[documentation](https://noirbizarre.github.io/projectd/) for the
configuration model (`projectd.toml`, `projectd.local.toml`), the built-in
LSP registry, and the daemon protocol.

## Documentation

<https://noirbizarre.github.io/projectd/>

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

MIT — see [LICENSE](LICENSE).
