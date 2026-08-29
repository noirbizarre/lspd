# PLAN

Tracks what has been built and what remains, by domain. Entries are numbered
`<domain>.<item>` and never renumbered — a completed item stays at its number
and gets checked, so a link or a conversation referencing `7.5` keeps meaning
the same thing next month. New items are appended to the end of their
domain's list, not inserted.

`[x]` done. `[ ]` not done — a gap, a deferred design decision (with its ADR
referenced where one exists), or genuinely future work.

## 1. Bootstrap & tooling

- [x] 1.1 Bootstrap from `rust.tpl` via `git-tpl` (single crate, `publish = false`, `docs = true`, `msrv = 1.88`)
- [x] 1.2 Add project dependencies (tokio, minijinja, notify, directories, toml, globset, lsp-types, rmcp, tracing, uuid, sha2, libc, schemars)
- [x] 1.3 Drop the Windows leg from the CI test matrix (Unix-only daemon — [ADR-0003](docs/adr/0003-unix-only-platform-scope.md))
- [x] 1.4 `typos.toml` exception for "projectd" itself
- [x] 1.5 Architecture-guard prek hook: the library never prints
- [ ] 1.6 Verify the MSRV (1.88) actually compiles — only built against a newer local toolchain so far; `msrv.yaml` has never run
- [ ] 1.7 Exercise `mise run cover` / Codecov upload end to end — never run against a real Codecov project
- [ ] 1.8 Validate the release pipeline (`gh ship validate`, a real tag) — no GitHub repo has been pushed to yet

## 2. Configuration engine

- [x] 2.1 Layered loading: registry → user → project → local (`config/layer.rs`)
- [x] 2.2 LSP definition patches with per-field merge (command/args replace, environment per-key, JSON deep-merge)
- [x] 2.3 Activation resolution: explicit list wins wholesale, zero-config auto-enable fallback
- [x] 2.4 Built-in registry: `pyright`, `ruff`, `rust-analyzer`, `gopls`, `typescript-language-server`
- [x] 2.5 Watch rule patches merged by name, same per-field rules
- [x] 2.6 `CommandSpec` string-or-array deserialization
- [ ] 2.7 No integration test exercises the user config layer (`$XDG_CONFIG_HOME/projectd/config.toml`) end to end — only project/local layers are covered
- [ ] 2.8 `[events].ignore` project config is documented (`docs/events.md`) but not implemented — only the hardcoded default ignore list exists
- [ ] 2.9 LSP `capabilities` is passed through as raw JSON only; the spec's "merged into ProjectD's default client capabilities" has no default set to merge into
- [ ] 2.10 `lsp-types` is a declared dependency but unused in code — integrate it (typed capabilities/initializationOptions) or remove it

## 3. Project detection

- [x] 3.1 Canonical root discovery (walk-up for `.git` / `projectd.toml`)
- [x] 3.2 Project type detection via marker files (rust/python/go/javascript/typescript)
- [ ] 3.3 Monorepo-aware detection (multiple project types in subdirectories) — out of scope by design (`project/detect.rs`), single-level markers only

## 4. Templating

- [x] 4.1 MiniJinja rendering of `command`/`args`/`environment`
- [x] 4.2 Recursive rendering of `settings`/`initializationOptions`/`capabilities` JSON blobs
- [x] 4.3 Strict undefined-variable behavior (a typo is a render error, not an empty string)
- [x] 4.4 `env.*` and `project.*` context
- [ ] 4.5 Extend the context beyond `env`/`project` (e.g. detected project types, Mise tool versions) — the seam exists, nothing else populates it yet

## 5. Mise provider

- [x] 5.1 `EnvironmentProvider` trait + `MiseProvider` (shells `mise env --json`)
- [x] 5.2 Fallback to the OS environment when `mise` is missing or fails
- [x] 5.3 `PROJECT_ROOT` always injected last, unshadowable
- [ ] 5.4 Non-blocking Mise invocation — currently a synchronous `std::process::Command` even inside the daemon's async context ([ADR-0007](docs/adr/0007-mise-as-pluggable-environment-provider.md))
- [ ] 5.5 A second provider implementation (e.g. Nix) — the trait seam exists, nothing implements it yet

## 6. Daemon & protocol

- [x] 6.1 Deterministic socket path derivation (SHA-256 of the canonical root)
- [x] 6.2 Versioned handshake (protocol name/version, instance id, capabilities)
- [x] 6.3 Newline-delimited JSON control protocol (`Request`/`Response`)
- [x] 6.4 Discover-or-start client lifecycle (spawn detached, own process group, log file)
- [x] 6.5 Graceful shutdown (SIGINT/SIGTERM/`Request::Shutdown`)
- [x] 6.6 Stale-socket detection and cleanup
- [ ] 6.7 `projectd daemon list` / cross-project enumeration of running daemons — deferred, no registry exists ([ADR-0004](docs/adr/0004-deterministic-socket-discovery.md))
- [ ] 6.8 `daemon --detach` CLI flag is parsed but ignored by `commands::daemon::serve` — wire it up or remove it
- [ ] 6.9 No push notification to already-attached clients when configuration changes — it is only re-resolved lazily on the next `ensure`/`endpoint` call

## 7. LSP lifecycle & proxy

- [x] 7.1 `ensure()`/`status()` process lifecycle, reusing running instances
- [x] 7.2 Per-instance dedicated proxy Unix socket, raw byte pass-through
- [x] 7.3 Single exclusive attached client per instance (a second connection is rejected — [ADR-0005](docs/adr/0005-single-exclusive-lsp-proxy-client.md))
- [x] 7.4 `lsp list`/`config` (daemon-independent) and `status`/`ensure`/`endpoint` (daemon-required) CLI commands
- [ ] 7.5 Lifecycle knobs (`auto_restart`, `max_restarts`, `shutdown_timeout_ms`) resolve into config but are never enforced — no crash-restart supervisor, no graceful-shutdown-then-kill sequence
- [ ] 7.6 No explicit `lsp stop`/terminate command — the only way to stop a running instance is stopping the daemon
- [ ] 7.7 True multi-client multiplexing of one LSP instance — deferred by design ([ADR-0005](docs/adr/0005-single-exclusive-lsp-proxy-client.md))

## 8. Filesystem events

- [x] 8.1 `notify` + `notify-debouncer-full` watcher, one per daemon
- [x] 8.2 Atomic rename detection (`file.renamed`), falling back to created+deleted
- [x] 8.3 Default ignore list (`.git`, `target`, `node_modules`, etc.)
- [x] 8.4 Server-side glob filtering via `Request::Subscribe { paths }`
- [x] 8.5 `projectd events` CLI command
- [ ] 8.6 Configurable ignore list per project — see 2.8
- [ ] 8.7 No event history/replay for a client that subscribes after the fact — `broadcast` only holds its channel capacity (1024), no persisted log

## 9. Watch rules

- [x] 9.1 `[[watch]]` schema (paths, events, command, enabled), merged by name
- [x] 9.2 Runtime execution against the shared event stream, non-overlapping runs per rule
- [x] 9.3 `watch list`/`enable`/`disable` CLI and protocol
- [ ] 9.4 Runtime enable/disable is session-only, never persisted — intentional for the MVP ([ADR-0002](docs/adr/0002-config-layering-and-merge-semantics.md)), but there is no opt-in way to persist it either
- [ ] 9.5 Watch rule `command`/`paths` are not passed through `TemplateContext`, unlike LSP definitions — `{{ env.* }}` in a `[[watch]]` command is inert

## 10. CLI

- [x] 10.1 Full command tree (status, project info/capabilities, lsp list/config/status/ensure/endpoint, events, watch list/enable/disable, task list/run, daemon, mcp)
- [x] 10.2 `--json` versioned envelope, `--project` override, `-v` verbosity
- [x] 10.3 Human-readable and JSON output for every command
- [ ] 10.4 Shell completion generation (`clap_complete` not wired up)
- [ ] 10.5 `--config` override for the user config path — only the XDG default location is ever consulted

## 11. MCP server

- [x] 11.1 `rmcp`-based stdio server, scoped to one project
- [x] 11.2 Tools: `project_info`, `project_capabilities`, `lsp_list`, `lsp_config`, `lsp_status`, `lsp_ensure`, `task_list`, `task_run`
- [x] 11.3 Structured JSON output (schemars-derived schemas)
- [ ] 11.4 HTTP/SSE transport — only stdio is wired up; `rmcp` supports more
- [ ] 11.5 MCP resources/prompts — not used, tools-only surface

## 12. Documentation

- [x] 12.1 `docs/architecture.md`, `configuration.md`, `lsp.md`, `events.md`, `protocol.md`, `mcp.md`
- [x] 12.2 9 ADRs (`docs/adr/0001`–`0009`)
- [x] 12.3 `README.md`, `AGENTS.md` rewritten with real project content
- [x] 12.4 `examples/` (python-dual-lsp, templated-settings, watch-rules)
- [ ] 12.5 `CHANGELOG.md` — generated by git-cliff on first release, does not exist yet
- [ ] 12.6 Rustdoc doc-tests / runnable usage examples on the public library API

## 13. Testing

- [x] 13.1 Unit tests across config/project/templating/mise/daemon modules (68)
- [x] 13.2 Integration tests: daemon protocol (5), events (4), watch (4), CLI end-to-end (7), MCP (4)
- [x] 13.3 Regression tests for the two real bugs found during development (`WatchList` internal-tagging panic, `file.*` wire spelling)
- [x] 13.4 Test hygiene: the real daemon the CLI end-to-end test spawns is shut down afterward
- [ ] 13.5 User-config-layer integration test — see 2.7
- [ ] 13.6 Crash/auto-restart behavior test — blocked on 7.5 not being implemented
- [ ] 13.7 Load/stress test of the LSP proxy or event stream under many subscribers

## 14. Release & distribution

- [x] 14.1 CI workflow (lint, test matrix on linux/macos, `gh ship validate`) adjusted for Unix-only
- [x] 14.2 Release workflow inherited from `rust.tpl` (prepare-release, publish-release, ship), targets set at bootstrap (linux amd64/arm64 gnu, darwin amd64/arm64)
- [ ] 14.3 First actual release/tag — no GitHub repo pushed yet
- [ ] 14.4 OS packaging (deb/rpm/AUR/Homebrew) — not started; explicitly left to the project by `rust.tpl`
