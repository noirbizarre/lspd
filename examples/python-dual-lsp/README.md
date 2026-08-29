# Python, two independently configured LSPs

The architecture's central validation case (spec §3, §26): `pyright` and
`ruff` both cover Python and are active simultaneously, each independently
configurable.

`pyproject.toml` alone would already auto-enable both (the built-in registry
marks them both `auto_enable = true` for `python`). `projectd.toml` here is
explicit anyway, to show the schema, and adds a `typeCheckingMode` override
that only touches `pyright`'s `settings` — `ruff`'s configuration is
untouched, and activation is unaffected either way.

```console
$ projectd project info
types: Python
enabled LSPs: pyright, ruff

$ projectd lsp config pyright
command: pyright-langserver --stdio
```

Try `projectd.local.toml` (e.g. `enabled = ["basedpyright", "ruff"]`,
uncommented) to see activation change without touching either definition.
