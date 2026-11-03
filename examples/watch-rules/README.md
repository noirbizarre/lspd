# Independent watch rules in a monorepo-shaped project

Two watch rules covering two different workspaces of the same language
ecosystem — the schema never models `language → watcher`, so a monorepo can
declare as many independent rules as it needs (spec §21).

```console
$ lspd watch list
* frontend-tests       frontend/**/*.ts
* backend-tests        backend/**/*.ts

$ lspd watch disable backend-tests
backend-tests: disabled
```

Editing a file under `frontend/` only ever triggers `frontend-tests`;
editing one under `backend/` only ever triggers `backend-tests` — each rule
matches independently against the same shared event stream (see
[Events and watch rules](../../docs/events.md)).
