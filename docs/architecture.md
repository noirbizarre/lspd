# Architecture

lspd centralizes project-scoped development infrastructure that editors,
terminal tools, and AI agents would otherwise each reimplement independently.

```text
                    lspd
                       │
              project-scoped services
                       │
          ┌────────────┼────────────┐
          │            │            │
       Neovim        Agent       other clients
          │            │            │
          └────────────┼────────────┘
                       │
                    shared LSPs
```

lspd composes existing standards rather than replacing them: LSP remains
LSP, MCP is an additional agent-facing interface, and [Mise](https://mise.jdx.dev)
provides tools and environments. See
[ADR-0007](adr/0007-mise-as-pluggable-environment-provider.md) for why Mise is
a provider, not a protocol dependency.

## The central separation: definitions vs activation

The architectural requirement that shapes everything else: **LSP definitions
are separate from LSP activation.**

```text
LSP definition
    ↓
named configuration

Project LSP activation
    ↓
list of names
```

A project may run multiple LSPs covering the same language — `pyright` and
`ruff` both cover Python, and both may be active simultaneously. Modeling
configuration as `language → LSP` or `project → one LSP per language` cannot
express this. Instead:

```toml
[lsp.pyright]
command = ["pyright-langserver", "--stdio"]
languages = ["python"]

[lsp.ruff]
command = ["ruff", "server"]
languages = ["python"]

[lsps]
enabled = ["pyright", "ruff"]
```

`[lsp.*]` tables and the `[lsps].enabled` list merge **independently** across
every configuration layer (see [Configuration](configuration.md)): a user can
override `pyright`'s settings without touching which LSPs are active, and a
project can change the active set without redefining any LSP. See
[ADR-0001](adr/0001-lsp-definitions-vs-activation.md).

## Component map

```text
config          registry, layering, merge, templating — pure, no daemon needed
project         canonical root discovery, project-type detection
mise            the pluggable environment provider
daemon          the control protocol, LSP lifecycle/proxy, fs events, watch rules
client          discover-or-start + request/response, shared by the CLI and MCP
mcp             tool definitions wrapping the same engine functions the CLI uses
cli / commands  argument parsing and human/JSON output (binary crate only)
```

`config` and `mise` are usable without a daemon at all — `lspd lsp config
<name>` resolves the effective configuration in-process. Only genuinely
stateful concerns (a running LSP process, a persistent filesystem watcher,
runtime watch-rule toggles) need the daemon. See
[Configuration](configuration.md) for the layering model and
[Protocol](protocol.md) for the daemon's wire format.

## Service lifecycle

A daemon's identity is the canonical project root, not a language or a
single LSP name — `project root + "pyright"` and `project root + "ruff"` are
distinct logical services, which is what makes multiple LSPs per language
normal rather than exceptional (spec §14–§15).

```text
discover(project)
      │
      ├── instance exists → connect
      │
      └── absent → ensure/start → connect
```

Discovery is a pure function of the canonical root (a truncated SHA-256 hash
becomes the socket filename — see
[ADR-0004](adr/0004-deterministic-socket-discovery.md)), so two clients in
the same project always compute the same socket path without asking
anything first. Handshake is a separate concept: once connected, a client
confirms protocol compatibility and reads the daemon's identity and
capabilities (see [Protocol](protocol.md)).

## Filesystem events are not an LSP watcher

```text
filesystem
    ↓
lspd event stream
    ├──→ LSP
    ├──→ Neovim
    ├──→ other IDEs
    ├──→ agents
    └──→ watch rules
```

lspd emits `file.created` / `file.modified` / `file.deleted` /
`file.renamed` events (with atomic renames, not delete+create pairs) to
every subscriber. Watch rules are just one more subscriber that happens to
run a command — they do not define the underlying watch mechanism, and
disabling every watch rule does not stop the event stream. See
[Events and watch rules](events.md) and
[ADR-0008](adr/0008-event-stream-independent-of-watch-rules.md).

## Non-goals

lspd does not implement package management, tool installation, an
environment manager, an IDE, an LSP client, or a generic build orchestrator.
Mise remains the tool/environment provider; a future provider (e.g. Nix)
would implement the same `EnvironmentProvider` seam without lspd
reimplementing either.
