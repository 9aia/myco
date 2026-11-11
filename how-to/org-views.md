---
url: /myco/how-to/org-views.md
---
# See work across repos

`myco view --source index` renders cross-repo views over the
**index universe** — a disposable derived cache (redb under
`$MYCO_HOME`, else `~/.local/share/myco`). The org is just a
view over the universe — every filter, grouping, and format the
view engine has applies to it.

## The universe

Inside a **lasius workspace** (`lasius.toml` found walking up
from the cwd) the universe is the workspace's clones —
`<root>/<repos_dir>/*` git checkouts, minus `exclude` globs —
unioned with `repos.toml` registrations outside the workspace
(`index list` marks each row `lasius`/`registry`). Outside
lasius the universe is the `repos.toml` registry alone, so
`myco index add` is still how repos join there.

The index dir holds:

* `repos.toml` — the registry (`[[repo]] path name`), a plain
  file you can inspect; inside lasius it also carries
  registrations outside the workspace
* `index.redb` — one `snapshots` table: repo path → JSON snapshot

It is a **derived, disposable cache** — markdown in each repo's
`project/` stays canonical. `rm -rf` the index dir and
`myco index rebuild` derives it back.

## Register repos

```sh
myco index list                       # universe + snapshot freshness
myco index rebuild [<path>]           # re-scan one or all
myco index add ~/GitHub/9aia/sepia    # explicit registration —
                                      #   required outside lasius
myco index add .                      # register the repo you're in
myco index remove ~/GitHub/9aia/sepia # unregister a repos.toml entry
```

Inside lasius every clone joins the universe automatically — no
`index add` needed. `add` still records an explicit registry
entry (and takes an initial snapshot); snapshots go stale as
ledgers change — `list` marks them `stale`; index-sourced views
auto-rebuild stale entries on read, or rebuild explicitly.

## Ledger-less repos — one org-wide ledger

Each universe repo has a ledger state — `local` (the
conventional reading), `none` (explicitly ledger-less), or
`org` (the org/global ledger: the *one* repo whose worktree
every ledger-less member resolves to).

The designation lives in the repo's own `myco.toml`:

```toml
[index]
ledger = "org"   # or "none" — "local" is the default
```

Resolution order: `myco.toml [index] ledger` → a legacy
`repos.toml` `ledger` key (`org`/`none`; `local` is the default,
not a mark — `myco index add --org` still writes it) →
derivation: a universe repo with no qualifying `project/`
worktree is `none`, one with a worktree `local`.

```sh
# myco.toml in the ledger repo:        [index] ledger = "org"
# or the registry equivalent:
myco index add ~/GitHub/9aia/project --org   # the one global ledger
```

Inside a `none` repo, verbs that need a project — `status`,
`todo add`, `view`, writes included — resolve to the org
ledger's worktree instead of failing. Repos outside the
universe still fail as before; `--dir`, `--root`, `MYCO_CONFIG`,
and `project_dir` keep top precedence. `none` entries are
omitted from index-sourced org views (no rows, no warnings),
report `no-ledger` under `index rebuild`/`index list`, and are
skipped by `org init` — the rollout never scaffolds `project/`
into them.

## Org views — `view --source index`

Two built-in presets run on the index universe:

```sh
myco view org-status                  # per-repo grouped list
myco view org-board                   # org kanban — repo-prefixed cards
myco view org-board --all             # every card — no row cap
myco view org-board --md              # markdown table for docs/READMEs
```

`org-status` is one `label (n)` bucket per registered repo — the
old `org status` digest's substitute. `org-board` merges every
repo's live items into the five lanes; cards are prefixed with
the repo name (`thal:T-118 …`) so one board spans the whole org.

Under the hood both are plain view specs with `source = "index"`
— the same thing works ad-hoc or in your own `[views]` config:

```sh
myco view --source index --group-by status --format board   # org-board minus its live-status filter
myco view --source index --group-by owner                   # who has what, org-wide
myco view org-board --kind issue --label bug                # filters still apply
myco view org-status --status open,claimed                  # active items only
```

```toml
# ~/.config/myco/myco.toml — a personal org preset
[views.hot-org]
source = "index"
filter = { pri_min = "high" }
group_by = "repo"
```

Items in an index-sourced view carry their repo tag: `group_by = "repo"` buckets by it, cards and list lines prefix `repo:ID`,
and `--json` items gain a `"repo"` field. Snapshots hold no
handoff state, so the `⚡` interrupted marker (and the
`"interrupted"` JSON flag) is repo-source only. A repo whose
worktree vanished keeps contributing its last snapshot and the
read warns on stderr.

## Board output

Human boards — `view`'s board format (`view board`, `view epic`,
`view org-board`, …) — are fitted to the terminal: sized to its
width (`$MYCO_WIDTH` overrides; 100 columns when piped) and
capped at a screenful of rows (~25) with a `… +N more` footer —
`--all` lifts the cap. `--md` and `--json` are unbounded. Output
is ANSI-colored on a tty: `NO_COLOR` disables color,
`CLICOLOR_FORCE`/`FORCE_COLOR` force it on.

On a tty the long views — boards, `index list` — page through
`$MYCO_PAGER`, else `$PAGER`, else `less -FRSX` (the `-F` means a
single screenful still exits instantly). An empty
`MYCO_PAGER`/`PAGER` disables paging; piped output always prints
directly.

For a fully interactive board — TODO/BACKLOG tabs, item preview,
done/defer/promote by key, including index-sourced `org-*` views
(`myco ui --view org-board`) — see
[`myco ui`](../getting-started.md#the-interactive-board).
