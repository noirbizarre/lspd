# ADR-0006: lspd resolves templates; clients never do

## Status

Accepted.

## Context

LSP `settings`/`environment` values may need project-specific data (e.g. an
absolute path built from the project root). The spec is explicit that
"clients must not need to independently ... render MiniJinja" — every
client that queries effective configuration should receive a value ready to
use.

## Decision

`config::EffectiveConfig::resolve_lsp` renders every template after merging
every configuration layer, before returning a `ResolvedLspDefinition`. The
context exposes `env.*` (the effective, Mise-resolved environment) and
`project.root`/`project.name`, with MiniJinja's undefined-variable behavior
set to `Strict` — a misspelled `env.*` reference is a render error, not a
silently empty string. Nothing downstream of `config::mod` — the CLI, the
MCP server, a future editor integration — ever sees `{{ }}` syntax.

## Consequences

- Adding a template reference to a new field only requires calling
  `TemplateContext::render`/`render_json` on it inside
  `EffectiveConfig::render_lsp` — no client-side work, ever, for any
  consumer of `lsp config`.
- Strict undefined-variable checking means a genuine typo fails loudly at
  resolution time (surfaced as `lspd::template::render`) instead of
  producing a subtly wrong path that only breaks the LSP process it get
  passed to.
- The context is deliberately small today. Extending it (e.g. exposing
  detected project types, or values from the Mise provider by name rather
  than only via `env.*`) is additive — existing templates keep working
  since MiniJinja resolves attribute access lazily against whatever the
  context actually contains.
