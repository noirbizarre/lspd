# Templated settings and environment

MiniJinja templates in `settings`/`environment` are rendered by lspd,
against the effective environment (OS env, overlaid with Mise's, with
`PROJECT_ROOT` always set last) — see
[Configuration](../../docs/configuration.md#templating).

```console
$ lspd lsp config pyright --json | jq '.settings.python.analysis.extraPaths'
["/absolute/path/to/this/example/src"]

$ lspd lsp config pyright --json | jq '.environment.PYTHONPATH'
"/absolute/path/to/this/example/src"
```

Neither value contains `{{ }}` syntax in the output — by the time a client
sees it, rendering has already happened.
