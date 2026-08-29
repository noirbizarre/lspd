# The daemon protocol

## Discovery

The control socket path is a pure function of the canonical project root —
no lookup registry, no PID file to go stale:

```text
socket = $XDG_RUNTIME_DIR/projectd/<sha256(root)[..8 bytes hex]>.sock
```

(falling back to `/tmp/projectd-<uid>/...` when `$XDG_RUNTIME_DIR` is unset).
Two clients resolving the same root always compute the same path. See
[ADR-0004](adr/0004-deterministic-socket-discovery.md).

A client that cannot connect spawns `projectd daemon --project <root>`
detached (its own process group, stdio redirected to a log file under
`$XDG_STATE_HOME/projectd/`), then retries the connection for up to 5
seconds — the "discover-or-start" lifecycle (spec §14).

## Handshake

The first exchange on every connection, before anything else is accepted:

```json
→ {"type":"handshake"}
← {"type":"handshake","protocol":"projectd","protocol_version":1,
   "projectd_version":"0.1.0","project_root":"/work/demo",
   "instance_id":"...", "capabilities":["lsp","events","watch"]}
```

`protocol_version` is checked by every client before sending anything else;
a mismatch is a `projectd::daemon::protocol_mismatch` error, not a garbled
parse. `instance_id` is fresh per daemon process, so a client can detect
"the daemon behind this socket restarted since I last talked to it."

## Wire format

Newline-delimited JSON, internally tagged (`{"type": "...", ...}`). Every
field added after protocol version 1 is `#[serde(default)]`, so an older
client's requests keep parsing against a newer daemon and vice versa.

| Request | Response |
|---|---|
| `handshake` | `handshake` |
| `lsp_status { name }` | `lsp_status` |
| `lsp_ensure { name }` | `lsp_ensured { name, status }` |
| `lsp_endpoint { name }` | `lsp_endpoint { socket }` |
| `subscribe { paths }` | switches the connection to push-only `event` frames until disconnect |
| `watch_list` | `watch_list { rules }` |
| `watch_set { name, enabled }` | `ack` |
| `shutdown` | `ack`, then the daemon shuts down |

Any request may instead receive `{"type":"error","code":"...","message":"..."}`
— a connection is never simply dropped on failure; a client always gets a
structured frame back.

!!! note "A lesson learned the hard way"
    An early version modeled `watch_list`'s response as a *newtype* variant
    wrapping a bare `Vec` (`WatchList(Vec<WatchRuleInfo>)`). Serde cannot
    internally-tag that shape — a JSON array has no object to merge a
    `"type"` key into — so it failed to *serialize* at runtime, closing the
    connection with a `malformed JSON message` error on the very first
    `watch list` call. It is now a struct variant
    (`WatchList { rules: Vec<WatchRuleInfo> }`), and
    `daemon::protocol::tests::every_response_variant_serializes_successfully`
    exists specifically so this class of bug fails a unit test instead of a
    live daemon.

## Filesystem events

```json
{"event":"file.created","path":"src/foo.py"}
{"event":"file.renamed","old_path":"src/foo.py","new_path":"src/bar.py"}
```

`subscribe` accepts an optional list of glob patterns (`paths`); an empty
list forwards everything. Filtering happens server-side — a client is never
sent an event only to discard it locally.

## LSP proxy sockets

`lsp_endpoint` returns a *separate* Unix socket, one per running LSP
instance, dedicated to raw byte pass-through — not part of the JSON control
protocol at all. See [LSP definitions and lifecycle](lsp.md#one-exclusive-client-at-a-time).

## MCP

`projectd mcp serve` speaks the standard MCP protocol (JSON-RPC over stdio,
via [`rmcp`](https://docs.rs/rmcp)) — it is a separate surface from the
control socket, not layered on top of it. See [MCP](mcp.md).
