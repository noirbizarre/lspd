# Examples

Example `lspd.toml`/`lspd.local.toml` configurations. lspd is a
CLI and daemon, not a library other crates depend on, so these are
configuration recipes to copy from rather than compilable Rust examples.

- [`python-dual-lsp/`](python-dual-lsp/) — the spec's own validation case:
  `pyright` and `ruff` both covering Python, active simultaneously, with a
  local override switching to `basedpyright`.
- [`templated-settings/`](templated-settings/) — MiniJinja templates in
  `settings`/`environment`, rendered against the effective environment.
- [`watch-rules/`](watch-rules/) — two independent watch rules in one
  project, one per workspace, proving watch rules are not modeled as
  `language → watcher`.

Run any of them with:

```console
$ cd examples/python-dual-lsp
$ lspd project info
$ lspd lsp list
```
