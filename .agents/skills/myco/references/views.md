# Views — view / report / index / org

Read-only lenses over the ledger. All accept `--json`; none mutate
(except `index` maintenance commands, which only touch the derived
cache — never the ledger).

## Single repo

```bash
myco status [--prompt]     # digest; --prompt is the token-sized agent form
myco view board            # kanban: Backlog | Open | Claimed |
                           #   In Progress | Blocked — fits the
                           #   terminal, top ~25 rows
myco view board --all      # every card, no row cap
myco view board --md       # same, as a markdown table (docs/READMEs)
myco view board --epic v2 --milestone 0.2   # filtered lanes
myco report                # aging + velocity + blocked — the trend
```

`view board` answers "what's in flight right now"; `report` answers
"how is the ledger trending" (stale items, throughput, stuck work)
plus "stale claims" — `in-progress` items idle >3d, interrupted in
practice. A `⚡` marks items whose latest handoff is an interrupt
(`myco interrupt`). Human boards shrink to the terminal width
(`$MYCO_WIDTH` overrides) and cap at ~25 rows — `--all` lifts the
cap; `--md`/`--json` always emit everything. List verbs take
`--limit N` for the same job. On a tty,
long output pages through `$MYCO_PAGER`/`$PAGER`/`less -FRSX`;
piped or agent output never pages.

## Groupings — `epic` / `milestone`

Items carry `--epic`/`--milestone` names (`[A-Za-z0-9_.-]+`, set at
create or via `item set`).

```bash
myco view epic             # board grouped by epic — one column per
                           #   epic, `ungrouped` trailing
myco view milestone        # same, grouped by milestone
myco view board --epic v2  # the lanes scoped to one epic
myco view board --milestone 0.2   # scoped to one milestone
```

`--md` emits the table form on any board-format view.

## Declarative views — `view`

One engine (filter → group → sort → limit → format) behind named
presets and ad-hoc flag specs. Builtins: `board`, `open`, `epic`,
`milestone`. Config adds `[views.<name>]` tables — repo `myco.toml`
presets are shared, `~/.config/myco/myco.toml` presets personal;
the layers merge by name (user wins; either shadows a builtin with
a warning). `myco check` lints repo-layer `[views]` only.

```bash
myco view                     # list defined views
myco view board               # the kanban
myco view my-preset           # a [views.my-preset] config spec
myco view --kind issue --status open --group-by owner --sort -created
myco view open --limit 5 --md # named spec + flag overrides merge
```

Flags merge over a named spec: a set flag replaces that field,
unset fields keep the spec's value. `--format list|board`;
`--live-only`/`--superseded` filter ADR chains. `myco ui --view
<name>` opens the TUI with that view's *filter* narrowing every
board (sort/group/format don't map onto columns).

## Cross-repo — `index` + `view --source index`

The index is a disposable derived cache (redb under
`~/.local/share/myco`, `$MYCO_HOME` overrides) — rebuild freely, the
markdown stays canonical.

```bash
myco index add <repo-path>     # register + snapshot (root or project/ path)
myco index list                # registered repos + snapshot freshness
myco index rebuild [<path>]    # re-scan one or all
myco index remove <path>       # unregister

myco view org-status           # per-repo grouped list (the old
                               #   `org status` digest's substitute)
myco view org-board            # org kanban, `repo:ID`-prefixed cards
myco view org-board --kind task          # any filter flag applies
myco view --source index --group-by status --format board   # ad-hoc org
```

`source = "index"` views auto-rebuild stale snapshots — register each
repo once with `index add`, then `view org-status` is the cheap
org-wide answer to "what am I working on everywhere". Index-sourced
items carry a `repo` tag: `group_by = "repo"` buckets by it, cards
prefix `repo:ID`, JSON items gain a `"repo"` field. Snapshots have no
handoffs — the `⚡`/`interrupted` flag is repo-source only.
`org init` (re-scaffold every registered repo) is the only `org`
subcommand left.

## When which

- Session start → `status --prompt` (see continuity.md).
- "Show me the work" / standup → `view board` or `report`.
- "What's left in this push" → `view board --epic <name>` /
  `view board --milestone <name>`.
- A filter/group you'll reuse → `[views.<name>]` + `myco view <name>`;
  one-off slices → `myco view --status … --group-by …`.
- Multiple repos in play → `index add` once per repo, then
  `view org-status` / `view org-board` (or any spec with
  `--source index`).
