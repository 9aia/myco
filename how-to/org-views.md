# See work across repos

`myco index` registers repos into a machine-wide cache;
index-sourced `myco view` specs render cross-repo views over it.
The org is just a view over the index universe — every filter,
grouping, and format the view engine has applies to it.

## The index

The index lives under `$MYCO_HOME`, else `$XDG_DATA_HOME/myco`,
else `~/.local/share/myco`:

- `repos.toml` — the registry (`[[repo]] path name`), a plain
  file you can inspect
- `index.redb` — one `snapshots` table: repo path → JSON snapshot

It is a **derived, disposable cache** — markdown in each repo's
`project/` stays canonical. `rm -rf` the index dir and
`myco index rebuild` derives it back.

## Register repos

```sh
myco index add ~/GitHub/9aia/sepia    # repo root or its project/ worktree
myco index add .                      # register the repo you're in
myco index list                       # repos + snapshot freshness
```

`add` takes an initial snapshot. Snapshots go stale as ledgers
change — `list` marks them `stale`; index-sourced views
auto-rebuild stale entries on read, or rebuild explicitly:

```sh
myco index rebuild                    # all registered repos
myco index rebuild ~/GitHub/9aia/sepia
myco index remove ~/GitHub/9aia/sepia # unregister + drop snapshot
```

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
myco view --source index --group-by status --format board   # = org-board
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

Items in an index-sourced view carry their repo tag: `group_by
= "repo"` buckets by it, cards and list lines prefix `repo:ID`,
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

For a visual single-repo board — TODO/BACKLOG tree, item detail,
per-kind tables — see
[`tamias-driver-myco`](../getting-started.md#the-interactive-board)
through pavo (`pavo view myco:board`).
