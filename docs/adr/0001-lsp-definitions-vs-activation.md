# ADR-0001: LSP definitions are separate from LSP activation

## Status

Accepted.

## Context

A project may need multiple LSPs covering the same language — Python
commonly runs both `pyright` (types) and `ruff` (linting/formatting)
simultaneously. Modeling configuration as `language → LSP` cannot express
this at all. Modeling it as `project → one LSP per language` forbids the
common case outright. Neither matches how editors themselves already treat
LSP server configuration (a named, reusable definition) versus the set of
servers a given client enables.

## Decision

Split the model into two independently mergeable concerns:

- **Definitions** (`[lsp.<name>]`): a stable, named configuration — command,
  arguments, environment, languages, settings, capabilities, lifecycle. A
  definition may cover multiple languages; multiple definitions may cover
  the same language.
- **Activation** (`[lsps].enabled`): a plain list of definition names, active
  for this project. It never redefines an LSP, only says which named
  definitions apply.

Both merge across the same configuration layers (built-in registry → user →
project → local), but by different rules: definitions merge by key (each
definition's own fields merge independently — see
[ADR-0002](0002-config-layering-and-merge-semantics.md)); the activation
list replaces wholesale, never appends.

## Consequences

- A project can enable `["pyright", "ruff"]` for the same language with no
  special-casing anywhere in the schema or the code.
- A user can override just `pyright`'s `settings` without touching
  activation, and a project-local override can change which LSPs are active
  without redefining any of them (spec §9's explicit requirement).
- Every LSP instance is identified by `project root + definition name`, not
  by language — `daemon::lsp_manager` never needs a "one server per
  language" invariant, and the built-in registry can register two
  independently auto-enabling Python entries (`pyright`, `ruff`) as the
  ordinary case, not an exception.
- The cost is one extra level of indirection: resolving "what runs" always
  requires two lookups (which names are active, then what each name
  resolves to) rather than one. In practice this collapses into
  `EffectiveConfig::resolve_lsp(name)` plus `EffectiveConfig::enabled()`, so
  the indirection is paid once, not by every caller.
