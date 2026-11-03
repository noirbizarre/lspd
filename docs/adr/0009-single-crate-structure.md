# ADR-0009: Single crate, not a Cargo workspace

## Status

Accepted.

## Context

lspd's scope spans configuration resolution, a daemon, an IPC protocol,
an MCP server, and a CLI — enough surface area that splitting it into
several crates (`lspd-config`, `lspd-protocol`, `lspd-daemon`,
`lspd-cli`) was considered, for stronger compile-time boundary
enforcement between them. However, `rust.tpl` — the template this project is
bootstrapped from and stays synchronized with via `git tpl update` — scaffolds
exactly one crate (`[lib]` + `[[bin]]`), not a workspace, and diverging from
that would mean re-deriving the template's toolchain/CI/release wiring for a
multi-crate layout it does not support.

## Decision

Stay a single crate. The library (`src/lib.rs`) holds every engine module
(`config`, `project`, `daemon`, `client`, `mcp`, `mise`, `templating`,
`output`, `error`); the binary crate holds only `cli.rs` (argument types) and
`commands/*` (argument-to-engine-call translation and human/JSON output) —
mirroring `AGENTS.md`'s inherited invariant that "nothing in the library
knows a command exists." Module boundaries within the library
(`config` never imports `daemon`; `daemon` depends on `config` but not
`mcp`) are enforced by code review and the natural direction of `pub use`
re-exports, not by the compiler.

## Consequences

- `git tpl update` continues to apply cleanly — the crate's shape never
  diverges from what the template scaffolds.
- Module boundaries are a discipline, not a compiler guarantee: nothing
  stops a future change from having `config` reach into `daemon` by
  mistake. This is an accepted tradeoff for a project of this size,
  revisited if the codebase grows enough that accidental coupling becomes
  a recurring problem in review.
- Integration tests (`tests/*.rs`) exercise the library directly
  (`lspd::daemon::serve`, `lspd::mcp::LspdServer`) rather than
  only through the compiled binary, which is what a single crate with a
  real `[lib]` target makes straightforward — `tests/cli.rs` is the one
  suite that additionally spawns the real binary, specifically to cover the
  subprocess discover-or-start path that in-process testing cannot reach.
