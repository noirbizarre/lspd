# ADR-0004: Deterministic socket-path discovery, no registry

## Status

Accepted.

## Context

Discovery must answer "where is the lspd instance for this project?"
Two designs were available: maintain a registry (a file mapping project
roots to socket paths, updated on daemon start/stop), or derive the socket
path directly from the project root so that discovery requires no lookup at
all.

A registry introduces its own consistency problem: it must be kept in sync
with which daemons are actually alive, cleaned up when a daemon crashes
without unregistering itself, and made safe for concurrent writers. None of
that problem exists if there is nothing to keep in sync.

## Decision

Derive the socket path as `$XDG_RUNTIME_DIR/lspd/<hash>.sock`, where
`<hash>` is the first 8 bytes of `SHA-256(canonical_root)` in hex. Discovery
is `control_socket_path(root)` — a pure function, computed identically by
every client and by the daemon itself at startup. "Is a daemon running" is
answered by attempting to connect, not by consulting any stored state; a
failed connection to an existing socket file is treated as staleness (the
file is removed before rebinding), not as an error requiring cleanup
elsewhere.

## Consequences

- No daemon registry to corrupt, no stale-entry cleanup logic, no need to
  reconcile "the registry says X but process X is dead."
- A daemon's log file location, proxy-socket directory, and control socket
  all derive from the same hash, so debugging one project's daemon means
  looking in one predictable place, computable from `sha256(root)` alone.
- The tradeoff: `lspd status` across *all* projects with a running
  daemon has no single place to enumerate them (there is no registry to
  list). This is accepted as out of scope for the MVP; a future
  `lspd daemon list` would need to scan `$XDG_RUNTIME_DIR/lspd/`
  for live sockets rather than read a manifest.
- Hash collisions are theoretically possible (64 bits of a well-distributed
  hash) but not practically a concern at the scale of "projects one
  developer machine has open at once."
