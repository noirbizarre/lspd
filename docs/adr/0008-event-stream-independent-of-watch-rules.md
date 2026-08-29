# ADR-0008: The event stream is independent of watch rules

## Status

Accepted.

## Context

It would be simpler, structurally, to build "watch rules" as the only
consumer of filesystem changes — have the watcher exist solely to run
commands. The spec explicitly rejects this: "Do not make the event system an
'LSP watcher'. It is a general project event stream," consumed by LSPs,
editors, agents, and watch rules alike, all as peers.

## Decision

`daemon::events::Watcher` owns the filesystem watcher and a `broadcast`
channel of normalized `FsEvent`s, with no knowledge of watch rules or
commands. `daemon::watch_runtime::WatchRuntime` is a separate component that
subscribes to that same broadcast channel like any other consumer (a
`projectd events` CLI invocation subscribes identically). Disabling every
watch rule does not touch the watcher; the event stream keeps running for
whichever other subscribers exist.

## Consequences

- Adding a new kind of event consumer (a future editor-integration push
  channel, say) means subscribing to the existing broadcast channel — no
  change to `daemon::events` and no interaction with watch rules at all.
- `WatchRuntime`'s tests (`tests/watch.rs`) exercise it against the real
  event stream, not a mocked one, because the two were never coupled enough
  to need mocking apart.
- The cost is one more moving part than a single combined
  "watcher-that-runs-commands" would have been — accepted because the spec
  treats this separation as load-bearing, not incidental.
