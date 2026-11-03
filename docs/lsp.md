# LSP definitions, activation, and lifecycle

## Definition schema

```toml
[lsp.pyright]
command = ["pyright-langserver", "--stdio"]   # or: command = "pyright-langserver", args = ["--stdio"]
languages = ["python"]
environment = { PYTHONPATH = "{{ env.PROJECT_ROOT }}/src" }
initialization_options = {}
settings = { python = { analysis = { typeCheckingMode = "basic" } } }
capabilities = {}

[lsp.pyright.lifecycle]
auto_restart = true      # default
max_restarts = 5         # default; 0 means unlimited
shutdown_timeout_ms = 5000 # default
```

`command` accepts either a plain string (the executable alone) or an array
(the spec's convention — element 0 is the executable, the rest are leading
arguments, prepended to whatever `args` also contributes). `settings` and
`initialization_options` are passed through using native LSP terminology
(`initializationOptions`, `workspace/configuration`) — lspd does not
invent its own vocabulary where an LSP concept already exists.

`auto_enable` (registry-only, not meaningful in project/user overrides) marks
a definition as a zero-config default — see
[Configuration](configuration.md#two-independently-merged-concerns).

## Effective configuration

```console
$ lspd lsp list
* pyright              python
* ruff                 python
  rust-analyzer        rust

$ lspd lsp config pyright
name:    pyright
command: pyright-langserver --stdio
languages: python
```

`lsp list` and `lsp config` never need a running daemon — they resolve the
same layered configuration `lspd status`/`project info` do, in-process.
Every field is already template-rendered; nothing downstream needs to know
about MiniJinja or Mise.

## Lifecycle and proxy access

`lsp status`, `lsp ensure`, and `lsp endpoint` do need a daemon — starting
one on demand if none is running for the project (spec §14). A definition
name identifies one logical service **per project**; `ensure` reuses an
already-running instance rather than starting a duplicate.

```console
$ lspd lsp ensure pyright
pyright: Running

$ lspd lsp endpoint pyright
/run/user/1000/lspd/<hash>/pyright.lsp.sock
```

The endpoint is a Unix socket dedicated to that one running instance,
carrying raw LSP stdio bytes (`Content-Length`-framed JSON-RPC, exactly as
the process itself speaks it) — lspd does not interpret LSP messages,
only pipes them.

### One exclusive client at a time

LSP is stateful: a server's `initialize`/`didOpen` sequence assumes one
client session. The MVP proxy therefore allows **one attached client per
running instance** — a second concurrent connection to the same endpoint is
closed immediately rather than queued or multiplexed. See
[ADR-0005](adr/0005-single-exclusive-lsp-proxy-client.md) for why, and what
would be required to relax this later.
