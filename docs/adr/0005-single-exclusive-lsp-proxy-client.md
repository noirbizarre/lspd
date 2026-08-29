# ADR-0005: One exclusive client per LSP proxy instance

## Status

Accepted.

## Context

The spec explicitly warns that LSP is stateful and that "not every LSP can
safely be shared arbitrarily" — an `initialize`/`didOpen`/`textDocument/*`
session assumes exactly one client on the other end of the connection.
Genuinely sharing one running LSP process across multiple clients (e.g. two
Neovim instances) requires an LSP-aware multiplexer: rewriting JSON-RPC
request IDs so concurrent callers do not collide, fanning out notifications
to every attached client, and reconciling divergent `didOpen`/`didChange`
state per client. That is a substantial LSP-protocol-aware component in its
own right.

## Decision

For the MVP, `daemon::lsp_manager` allows exactly one attached client per
running LSP instance. `lsp_manager::run_proxy` tracks an `attached` flag per
instance; a second concurrent connection to the same proxy socket is closed
immediately rather than queued, error-framed, or multiplexed. Ownership of
*when* to start/stop the process still belongs to projectd (spec's
requirement); only simultaneous *use* by multiple clients is out of scope.

## Consequences

- The proxy implementation is a plain bidirectional byte copy
  (`tokio::io::copy` in both directions) — no JSON-RPC parsing, no ID
  rewriting, no per-client state tracking on the daemon side.
- Two editors (or an editor and an agent) wanting the same LSP simultaneously
  each get their own attempt rejected until the other disconnects — this is
  a real, user-visible limitation, not a hidden one; it is documented here
  and in [LSP definitions and lifecycle](../lsp.md).
- A future multiplexing implementation would sit entirely inside
  `lsp_manager`, behind the same `endpoint()`/proxy-socket interface — no
  protocol change would be needed for `client`, `commands::lsp`, or
  `mcp::tools`, since none of them are aware of how many clients a proxy
  socket permits.
