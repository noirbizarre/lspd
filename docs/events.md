# Events and watch rules

## The event stream

lspd owns one shared filesystem watcher per project (backed by
[`notify`](https://docs.rs/notify), abstracting inotify/FSEvents/etc.), and
broadcasts normalized events to every subscriber:

```text
file.created    { path }
file.modified   { path }
file.deleted    { path }
file.renamed    { old_path, new_path }
```

Renames are atomic whenever the OS backend can pair them; otherwise they
surface as an ordinary `deleted` + `created` pair. Rapid-fire events (an
editor's atomic save-via-rename) are debounced into one logical event per
change.

Default-ignored directories: `.git`, `target`, `node_modules`, `.venv`,
`venv`, `__pycache__`, `dist`, `build`, `.lspd`.

```console
$ lspd events --path '**/*.py'
Created { path: "src/new_module.py" }
```

`events` requires a daemon (starting one if needed) — the whole point of a
*shared* watcher is that this command does not run its own.

## Watch rules are a consumer, not the source

```text
filesystem
    ↓
lspd event stream
    ├──→ LSP
    ├──→ editors, agents
    └──→ watch rules   ← just one more subscriber
```

A watch rule matches events by glob and kind, and runs a command when they
occur — nothing more:

```toml
[[watch]]
name = "frontend-tests"
paths = ["frontend/**/*.ts"]
events = ["file.modified"]
command = ["mise", "run", "test:frontend"]
```

Rules merge across configuration layers by `name`, exactly like LSP
definitions (see [Configuration](configuration.md)). A monorepo may declare
several independent rules for the same language without conflict — the
schema deliberately never models `language → watcher`.

While a rule's command is still running, a further matching event for that
*same* rule is skipped (logged, not queued) rather than starting a second
overlapping run. Different rules run independently and concurrently.

```console
$ lspd watch list
* frontend-tests       frontend/**/*.ts

$ lspd watch disable frontend-tests
frontend-tests: disabled
```

`watch enable`/`watch disable` change the running daemon's in-memory state
only — they are not written back to any configuration file. Restarting the
daemon resets every rule to its configured `enabled` value. See
[ADR-0002](adr/0002-config-layering-and-merge-semantics.md) for why this is
intentional rather than a shortcut.

Mise tasks may be — and are, in the example above — the commands watch rules
run. lspd does not replace Mise's task system, and does not implement
dependency-graph-aware build orchestration (no NX-like scheduler in this
MVP).
