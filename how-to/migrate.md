---
url: /myco/how-to/migrate.md
---
# Migrate a legacy ledger

`myco migrate` brings pre-v2 ledgers up to the current spec.
Two modes, auto-detected by what's on disk.

## `pm/` → `project/`

A `pm/` directory at the repo root is the pre-worktree layout.
One command converts and removes it:

```sh
cd your-repo
myco migrate
```

* `pm/TODO.md` / `pm/BACKLOG.md` bullets become `T-` items in the
  new `TODO.md` / `BACKLOG.md` — done items are dropped
* `pm/ROADMAP.md` becomes a `plans/` file
* any other `pm/*.md` becomes an `issues/` file
* a fresh `project/` worktree is scaffolded (the `init` path)
* `pm/` is removed and its deletion committed on the code branch

## Legacy `project/` → v2

Files already on a `project` branch are normalized in place:

* id-less items get ids
* `[x]`/done items drop (history keeps them)
* heading-like lines become `##` sections
* frontmatter-less files in item dirs are *adopted* — the
  filename's number is honored when free (`0001-foo.md` →
  `ADR-0001`); collisions take the next unclaimed number with a
  warning. Adopted files get synthesized frontmatter and the
  canonical name
* everything is re-emitted canonically

## After migrating

```sh
myco check        # should report clean
myco status       # the ledger, v2-shaped
```

`--json` on `migrate` returns the run's counters — items
converted, ids allocated, files moved, warnings. If the run left
unparsed lines, they're listed there; fix by hand and `check`
again.
