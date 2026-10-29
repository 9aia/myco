---
url: /myco/how-to/import-work.md
---
# Import external work

`myco absorb` pulls work that lives elsewhere into the ledger —
code comments, Jira exports, GitHub issue dumps.

## Sweep code comments

```sh
myco absorb comments                  # the repo root
myco absorb comments --path src/ --kinds TODO,FIXME,HACK
myco absorb comments --to issues      # file issues instead of backlog items
myco absorb comments --dry            # report without creating
```

Uses ripgrep (`rg` must be installed), respects `.gitignore`, and
skips `project/`, `docs/`, `target/`. Findings land as `T-` items
— `BACKLOG.md` by default, `issues/` files with `--to issues`.

## Jira CSV

```sh
myco absorb jira export.csv
myco absorb jira export.csv --include-done
```

`Done`/`Resolved` rows are skipped unless `--include-done` — then
they're created `done` and archived.

## GitHub issue dump

```sh
myco absorb github dump.json          # or dump.csv
myco absorb github dump.json --include-done
```

JSON input is an array of issue objects — a
`gh issue list --json number,title,state,labels,assignees,body`
dump works as-is (`labels` entries can be `"name"` strings or
`{name:…}` objects, `assignee`/`assignees` strings or
`{login:…}` objects; extra fields are ignored). CSV is the
equivalent columns. `closed` issues are skipped unless
`--include-done` (created `done` + archived).

## After importing

```sh
myco status         # the ledger, now bigger
myco board          # or the kanban view
myco check          # sanity
```

Each absorb run is one commit — `--no-commit` batches several
imports into one `myco commit`.
