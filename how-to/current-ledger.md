---
url: /myco/how-to/current-ledger.md
---
# Pin the ledger myco opens

`myco` discovers the ledger from where you stand — but sometimes
you want it to open a *different* one: the org ledger from a scratch
folder, a sibling repo's board, or one pinned ledger everywhere.
This guide covers the selector surfaces; [ledger selection](/explanation/ledger-selection)
explains the model and precedence.

## See what would open

```bash
myco current show        # env, persisted [current], pointer, resolved ledger
myco current show --json # machine-readable
```

## Pin a ledger globally

```bash
myco current use org              # the index-designated org ledger
myco current use myrepo           # a universe repo by name
myco current use /abs/path/project # a worktree or repo root
myco current use org --mode always # beat cwd discovery everywhere
```

`--mode` is `backup` (default — answers only when discovery finds
nothing) or `always` (wins over the repo you're standing in).
`myco current use` validates the target immediately — a bad pin
fails loudly instead of surprising you later.

This writes `[current]` to `~/.config/myco/myco.toml`:

```toml
[current]
ledger = "org"
mode = "backup"
```

Hand-editing works too — same file, same keys. A repo's own
`myco.toml` can never set this; the selector is user-layer only.

## Point one folder at a ledger

```bash
cd ~/scratch
myco current use org --here   # writes ./.myco.toml
myco current clear --here    # removes it
```

`.myco.toml` holds a single `ledger = "…"` key and is honored only
outside git repositories — inside a repo it's ignored (a committed
pointer would be repo-controlled injection). It has fallback
semantics: a repo's own ledger always wins over it.

## Per-invocation overrides

```bash
MYCO_LEDGER=org myco status --prompt
MYCO_LEDGER=myrepo MYCO_LEDGER_SCOPE=backup myco ui
myco --dir /path/to/project todo list
```

`MYCO_LEDGER_SCOPE` defaults to `always`; `MYCO_LEDGER=repo`
disables the env override for that invocation — the escape hatch
when a persisted pin went stale.

## Scope a set of repos (workspace)

Index-sourced views (`myco view org-board`, `org-status`, and
`myco ui --view org-*`) read the whole universe by default.
`[workspace]` narrows them to a glob-selected set:

```bash
myco workspace list              # universe repo names to glob over
myco workspace use 'my*' 'org'   # persist the globs
myco workspace show              # globs → resolved set + warnings
myco workspace clear             # back to the full universe
```

```toml
[workspace]
repos = ["my*", "org"]
```

A glob matching nothing warns; a set matching nothing fails.
Single-ledger verbs are unaffected — the workspace scopes views,
not which ledger `status` opens.
