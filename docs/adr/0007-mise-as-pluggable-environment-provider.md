# ADR-0007: Mise as a pluggable environment provider

## Status

Accepted.

## Context

LSP processes and watch-rule commands need the project's effective
development environment (tool paths, versions, project-specific variables).
[Mise](https://mise.jdx.dev) already solves tool installation, version
management, and environment resolution; reimplementing any part of that
inside lspd would duplicate a maintained tool and inevitably drift from
it. The spec is explicit that Mise is "an implementation/provider detail,
not part of the LSPD protocol," anticipating other providers (e.g. Nix)
later.

## Decision

Define a small `mise::EnvironmentProvider` trait
(`fn resolve(&self, root: &Path) -> HashMap<String, String>`) and implement
it once as `MiseProvider`, which shells out to `mise env --json` and merges
its output over the process's own environment (Mise wins on overlap).
`mise::effective_environment` always injects `PROJECT_ROOT` last, so neither
Mise nor the OS environment can shadow lspd's own contribution. If the
`mise` binary is missing or exits non-zero, `MiseProvider` logs a warning and
falls back to the plain OS environment rather than failing — a project that
does not use Mise must still work.

## Consequences

- `config::EffectiveConfig::resolve_with` takes the provider as a parameter,
  so unit tests substitute a stub provider (`config::tests::StubProvider`)
  instead of shelling out to a real `mise` binary on every test run.
- Adding a Nix provider later means implementing the same trait and wiring
  it in as an alternative to `MiseProvider` — no change to `config`,
  `templating`, or the daemon, all of which only depend on the trait.
- The current implementation shells out synchronously (`std::process::Command`)
  even from async contexts (the daemon's startup path). For a one-shot CLI
  invocation this is harmless; for the daemon it means config resolution —
  which happens once at startup and again per `lsp ensure`/`lsp endpoint`
  call — briefly blocks that call. This was measured as acceptable for the
  MVP's request volume; moving it to `tokio::task::spawn_blocking` is
  possible without changing the trait if it ever needs to.
