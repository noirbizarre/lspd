# AGENTS.md

Notes for anyone — human or otherwise — changing this repository.

## What this project is

Project-scoped development infrastructure: shared LSPs, filesystem events and MCP for editors and agents

projectd is a local daemon, spawned on demand per project, that shares LSP
server processes, a filesystem event stream, and a Mise-resolved environment
across every client that asks for them — an editor, a terminal tool, an AI
agent — over a Unix socket and a minimal MCP server. It composes existing
standards (LSP stays LSP, Mise stays the tool/environment provider) rather
than replacing them. See `docs/architecture.md` and `docs/adr/` for why it
is shaped the way it is before proposing a different shape.

## Non-negotiable invariants

Each of these should be enforced by a hook or a test. An invariant nothing
checks is a comment, and it will be violated.

1. **LSP definitions and LSP activation merge independently.** A project can
   override one definition's `settings` without touching which LSPs are
   active, and change activation without redefining any LSP — enforced by
   `config::tests::project_config_can_override_one_definition_without_touching_activation`
   and `config::tests::local_config_can_change_activation_without_redefining_any_lsp`.
2. **A language is never a service identity.** Multiple LSP definitions may
   cover the same language and run simultaneously — enforced by
   `config::tests::both_pyright_and_ruff_resolve_to_distinct_independently_configured_definitions`
   and the built-in registry shipping two independent Python entries.
3. **Every daemon `Response` variant must actually serialize.** An internally
   tagged newtype variant wrapping a bare sequence compiles but panics at
   runtime — this exact bug shipped once during development. Enforced by
   `daemon::protocol::tests::every_response_variant_serializes_successfully`;
   a round-trip test alone would not have caught it.
4. **The control protocol's wire spelling matches the spec's `file.*`
   convention**, not whatever `#[serde(rename_all = "snake_case")]` would
   otherwise produce — enforced by
   `daemon::events::tests::a_renamed_event_serializes_to_the_documented_file_dot_renamed_shape`
   and `config::watch::tests::event_kind_parses_from_the_documented_file_dot_prefixed_spelling`.
5. **Templates are resolved by projectd, never by a client.** `lsp config`'s
   output never contains `{{ }}` syntax — enforced by
   `config::tests::settings_templates_render_against_the_effective_environment`.
6. **The library never imports `cli` or prints anything.** `commands/*` (the
   binary crate) is the only place argument parsing meets engine calls.

## Layout

```
src/
├── lib.rs        the library surface: config, project, daemon, client, mcp, mise, templating
├── main.rs       the projectd binary — thin: parse, dispatch, map exit code
├── cli.rs        argument types only
├── error.rs      the crate's error type
├── config/       layered configuration, merge rules, the built-in registry
├── daemon/       the control protocol, LSP lifecycle/proxy, fs events, watch rules
├── project/      canonical root discovery, project-type detection
├── mcp/          the MCP server, wrapping the same engine calls the CLI uses
└── commands/     (binary crate) argument-to-engine-call translation, human/JSON output
```

Dependencies point inward. Nothing in the library knows a command exists.

## Style

**Every non-obvious line carries a comment saying why.** Not what — the code
says what. Ideally naming the failure it prevents. A comment that restates the
code is worse than none.

**Errors are typed and actionable.** `thiserror` for the library, `miette` at
the binary edge. A diagnostic must carry the two things the user does not
already know: what specifically failed, and what to do about it. Diagnostic
codes are `projectd::<module>::<kind>`, and a code is a public identifier
users grep for — renaming one is a breaking change.

**Test names are sentences.** `an_unchanged_input_produces_no_output`, not
`test_run_2`. The name should say what would be broken if it failed.

## Commits

Conventional Commits, enforced by commitlint on `commit-msg`. The type becomes a
changelog heading, so choose it as if someone will read it in release notes —
because they will.

## Releases

Driven by gh-ship. Never bump a version or push a tag by hand: `cliff.toml`
derives the version from the commit history, `prepare-release` applies it, and
`.github/ship.yml` is the contract between them. See CONTRIBUTING.md.

## Before you push

```sh
mise run ci
```

Formatting, Clippy, spelling, workflow linting, tests and the documentation build. Same as CI.

## This repository is generated from a template

The toolchain, hooks, CI and release workflows come from
[rust.tpl](https://github.com/noirbizarre/rust.tpl) and are updated with
`git tpl update`. Files carrying template-owned content end with a
`# --- project-specific ---` marker: add below it, never above.

Changing template-owned content here fixes it in one repository. Changing it in
the template fixes it in all of them — prefer that.
