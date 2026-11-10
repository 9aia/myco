---
url: /myco/how-to/init-repo.md
---
# Set up myco on a repository

`myco init` turns any git repo into a myco project: an orphan
`project` branch checked out as a `project/` worktree.

```sh
cd your-repo
myco init
```

## What it creates

| path                                                            | what                                                           |
| --------------------------------------------------------------- | -------------------------------------------------------------- |
| `project/`                                                      | git worktree on the orphan `project` branch                    |
| `project/meta.toml`                                             | repo name + per-kind id counters (`[next]`)                    |
| `project/TODO.md`                                               | active tasks                                                   |
| `project/BACKLOG.md`                                            | deferred items                                                 |
| `project/DONE.md`                                               | done archive — completed tasks stay browsable                  |
| `project/index.md`                                              | human-readable ledger index                                    |
| `project/issues/`, `proposals/`, `plans/`, `adrs/`, `handoffs/` | file-backed item dirs                                          |
| `myco.toml`                                                      | repo-root config — seeded with the commented `[[kinds]]` prelude (only when absent) |
| `.gitignore` += `project/`                                      | keeps the worktree out of code commits                         |
| `AGENTS.md`                                                     | gains a marker-delimited, myco-managed *Agent awareness* block |

`init` commits the scaffold on the `project` branch — your code
branch gains `myco.toml` (the commented kinds prelude, only when
the file doesn't already exist), the `.gitignore` line, and the
`AGENTS.md` block. The block teaches agents the myco workflow; it's delimited
by `<!-- myco:agent-awareness -->` markers so prose elsewhere in
the file is preserved — a hand-written *Agent awareness* section
is upgraded in place. `check` validates it as an
`agents-md` finding and `check --fix` repairs it.

## Re-running init

`init` is idempotent: a valid existing worktree gets its scaffold
verified/repaired instead of re-created. It is the safe "is this
repo set up?" probe. `myco org init` re-runs it on every
registered repo — the rollout path when scaffolding (like the
`AGENTS.md` block) is introduced or updated. Repos registered
`ledger = "none"` (intentionally ledger-less org members) are
skipped — see [org views](org-views.md#ledger-less-repos--one-org-wide-ledger).

## Overrides and discovery

`--root` picks the repo, `--dir` picks a non-default worktree
path (init uses it instead of `<root>/project`):

```sh
myco init --root ~/GitHub/9aia/sepia
myco init --dir ~/work/ledger          # worktree somewhere else
```

Every other verb discovers the project the same way, in order:

1. `--dir` — explicit worktree path (must qualify as a myco project)
2. `--root` — repo root (worktree is `<root>/project`)
3. cwd inside the project worktree itself
4. `project_dir` in `myco.toml` (see [config](../reference/config.md))
5. walk up from cwd for a `project/` worktree on branch `project`
6. org-ledger fallback — a *registered* repo with no ledger of its
   own resolves to the org ledger (see
   [org views](org-views.md#ledger-less-repos--one-org-wide-ledger))

A non-standard worktree location stays discoverable if you set
`project_dir` (relative to the repo root) in a `myco.toml` —
the search order is `$MYCO_CONFIG` → `./myco.toml` (where you
run `myco`) → `~/.config/myco/myco.toml`.

## If the repo has a `pm/` dir

Don't `init` — run [`myco migrate`](migrate.md) instead. It
converts the legacy `pm/` layout into a fresh `project/` worktree
in one pass.
