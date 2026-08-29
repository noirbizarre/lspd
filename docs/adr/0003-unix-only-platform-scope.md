# ADR-0003: Unix-only platform scope

## Status

Accepted.

## Context

The spec requires Unix domain sockets for local discovery ("do not use TCP
ports"). `tokio::net::UnixListener`/`UnixStream` are gated `#[cfg(unix)]` and
do not exist on Windows. The daemon also relies on process groups
(`std::os::unix::process::CommandExt::process_group`) to detach a spawned
daemon from its parent's terminal/signal group, and on `libc::getuid` for the
`/tmp` runtime-directory fallback.

## Decision

Target Linux and macOS only for the MVP. No `#[cfg(windows)]` fallback path,
no TCP-socket alternative transport, no attempt to keep the crate compiling
on Windows. `rust.tpl`'s default CI test matrix (`ubuntu`/`windows`/`macos`)
is edited to drop the Windows leg, and the release target list
(`data/targets.toml` at `git tpl init` time) excludes every Windows triple.

This is a permanent, deliberate deviation from the template's defaults —
`git tpl update` will keep re-adding the Windows CI leg from upstream, and
the correct response each time is to remove it again, not to make the crate
build there.

## Consequences

- The whole daemon/socket/proxy/event-stream subsystem is unconditionally
  Unix-only code — no cross-platform abstraction layer to maintain, no
  Windows-specific code paths to test.
- Windows users cannot run projectd at all in this MVP, including the
  daemon-independent commands (`lsp config`, `project info`) that have no
  technical reason to require Unix — they are blocked only by sharing a
  binary with the Unix-only parts. Splitting them into a
  Windows-compatible subset was considered and rejected as unnecessary
  complexity until real Windows demand exists.
- Revisiting this later means introducing a second discovery/IPC transport
  (e.g. named pipes) behind the same `client`/`daemon::socket` seam, which
  does not yet exist as an abstraction — it would need to be designed at
  that point, not retrofitted onto assumptions made here.
