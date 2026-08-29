# ADR-0002: Hand-rolled layered configuration with per-field merge rules

## Status

Accepted.

## Context

Configuration must merge across four layers (built-in registry, user,
project, local) with rules that differ *per field*: LSP definitions merge by
key; `settings`/`initializationOptions`/`capabilities` deep-merge as JSON;
`environment` merges per-key; the activation list and most watch-rule fields
replace wholesale. Generic layered-config crates (`figment`, `config-rs`)
provide one recursive merge strategy for an entire document; bending them to
express per-field exceptions (map-merge here, replace there, deep-JSON-merge
elsewhere) would mean writing custom `Provider`/merge logic on top of them
anyway.

## Decision

Parse every layer into a "patch" type — every field `Option<T>` (or an
empty/absent collection), meaning "this layer did not say" — using plain
`serde` + `toml`. Fold patches from lowest to highest priority with a small,
explicit `Merge` per field (`config::merge`), then resolve the fully merged
patch into a concrete type, validating required fields (a `command` must
exist somewhere) only at that point.

No generic layered-config crate is used. Rendering MiniJinja templates
happens after merging, against a resolved patch, so a partial override in a
higher layer can depend on the environment without needing to re-specify
fields it did not intend to change.

## Consequences

- Every merge rule is a plain, individually testable function
  (`config::merge::{replace, merge_maps_by_key, merge_json, merge_env}`) —
  the semantics are documented in code, not inferred from a library's
  default behavior.
- Runtime overrides that must **not** persist to disk (watch rule
  enable/disable via `projectd watch enable/disable`) live entirely outside
  this patch/merge pipeline, in `daemon::watch_runtime`'s in-memory state,
  resetting to the configured default on daemon restart. This is a
  deliberate simplification for the MVP, not an oversight: persisting
  runtime toggles would require deciding which file to write them into and
  how that interacts with a committed `projectd.toml`, which is unresolved
  scope.
- Adding a new mergeable field means writing (and testing) its merge rule
  explicitly — more code than a one-line schema change in a generic
  library, in exchange for merge behavior that is never a surprise.
