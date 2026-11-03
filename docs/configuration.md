# Configuration

lspd works with zero configuration: a project with no `lspd.toml` at
all still gets sensible LSPs activated, based on the built-in registry and
detected project markers (`Cargo.toml`, `pyproject.toml`, `go.mod`,
`package.json`). Configuration files only ever *override* that default.

## Layers

```text
built-in registry                       (compiled in, never a file on disk)
    ↓
$XDG_CONFIG_HOME/lspd/config.toml   (user)
    ↓
lspd.toml                           (project, optional, may be committed)
    ↓
lspd.local.toml                     (project-local, normally untracked)
    ↓
template/environment resolution
    ↓
effective configuration
```

Higher layers override lower ones. A layer that does not exist contributes
nothing — this is how zero-config projects work structurally rather than as
a special case.

Add to your project's `.gitignore`:

```gitignore
lspd.local.toml
```

## Two independently merged concerns

**LSP definitions** (`[lsp.<name>]`) merge **by key**, and each definition's
fields merge independently:

| Field | Merge rule |
|---|---|
| `command`, `args` | later layer replaces wholesale |
| `environment` | merged per key, later layer wins per key |
| `languages` | later layer replaces wholesale |
| `initializationOptions`, `settings`, `capabilities` | deep JSON merge (objects merge recursively; arrays and scalars replace) |
| lifecycle knobs | each field independently replaceable |

This is why the spec's example works:

```toml
# lspd.toml — defines pyright's command
[lsp.pyright]
command = ["pyright-langserver", "--stdio"]

# $XDG_CONFIG_HOME/lspd/config.toml — a user overriding only a setting
[lsp.pyright.settings.python.analysis]
typeCheckingMode = "strict"
```

The user's file never mentions `command`, so `pyright`'s executable still
comes from `lspd.toml` — only the `settings` object gains
`typeCheckingMode`, deep-merged into whatever was already there.

**LSP activation** (`[lsps].enabled`) is a plain list that **replaces
wholesale**, never merges by appending:

```toml
# lspd.toml
[lsps]
enabled = ["pyright", "ruff"]

# lspd.local.toml — a developer trying basedpyright locally
[lsps]
enabled = ["basedpyright", "ruff"]
```

If **no** layer sets `[lsps].enabled` at all, lspd falls back to
zero-config auto-activation: every built-in registry entry whose `languages`
matches a detected project type and whose (registry-only) `auto_enable` flag
is `true`. Setting `[lsps].enabled` anywhere, even to an empty list, disables
this fallback entirely — explicit configuration always wins outright, never
partially blends with the implicit default.

**Watch rules** (`[[watch]]`) merge the same way as LSP definitions: each
rule's `name` is the merge key, and a rule's fields (`paths`, `events`,
`command`, `enabled`) each replace wholesale when a later layer sets them.

## Templating

Every string value in `environment`, and every string leaf inside
`settings`/`initializationOptions`/`capabilities`, may contain a
[MiniJinja](https://docs.rs/minijinja) template:

```toml
[lsp.pyright.settings.python.analysis]
extraPaths = ["{{ env.PROJECT_ROOT }}/src"]
```

Templates are rendered **by lspd**, after every layer has merged, using
the *effective* environment (the OS environment, overlaid with whatever
[Mise](https://mise.jdx.dev) reports via `mise env --json`, with
`PROJECT_ROOT` always set to the canonical project root last — no layer can
shadow it). Clients receiving `lspd lsp config <name>` see only the
rendered result; they never need their own templating engine.

A misspelled `env.*` reference is a render error, not an empty string — see
[ADR-0006](adr/0006-server-side-template-resolution.md).

The context is intentionally small today (`env.*`, `project.root`,
`project.name`) but designed to grow: new namespaces can be added without
breaking existing templates.

## The built-in registry

Ships with definitions for `pyright`, `ruff`, `rust-analyzer`, `gopls`, and
`typescript-language-server` (source:
[`src/config/registry/builtin.toml`](https://github.com/noirbizarre/lspd/blob/main/src/config/registry/builtin.toml)).
Python intentionally has two independently defined, independently
auto-enabling entries — the minimal case the architecture must support (see
[Architecture](architecture.md)).
