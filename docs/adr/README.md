# Architecture Decisions

Records of the decisions that shape this project, and — more usefully — the
reasons behind them. An ADR is written when a choice is hard to reverse or
likely to be re-proposed.

The point is not the decision; it is the alternatives that were rejected and
why. A record that only states the outcome saves nobody the argument.

A decision is changed by writing a new ADR that supersedes the old one, never
by editing the old one. The history is the value.

## Format

`NNN-kebab-case-title.md`, numbered in the order written, with the sections:

- **Status** — Proposed, Accepted, or Superseded by ADR-NNN
- **Context** — the forces in play, before any decision
- **Decision** — what was decided
- **Consequences** — what this costs, including what it makes harder

## Index

- [ADR-0001](0001-lsp-definitions-vs-activation.md) — LSP definitions are separate from LSP activation
- [ADR-0002](0002-config-layering-and-merge-semantics.md) — Hand-rolled layered configuration with per-field merge rules
- [ADR-0003](0003-unix-only-platform-scope.md) — Unix-only platform scope
- [ADR-0004](0004-deterministic-socket-discovery.md) — Deterministic socket-path discovery, no registry
- [ADR-0005](0005-single-exclusive-lsp-proxy-client.md) — One exclusive client per LSP proxy instance
- [ADR-0006](0006-server-side-template-resolution.md) — lspd resolves templates; clients never do
- [ADR-0007](0007-mise-as-pluggable-environment-provider.md) — Mise as a pluggable environment provider
- [ADR-0008](0008-event-stream-independent-of-watch-rules.md) — The event stream is independent of watch rules
- [ADR-0009](0009-single-crate-structure.md) — Single crate, not a Cargo workspace
