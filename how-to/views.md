# Shape the board with named views

A view is a declarative spec — `filter`, `group_by`, `sort`,
`limit`, `format` — evaluated by one engine. Names come from two
places: built-in presets (`board`, `open`, `epic`, `milestone`,
`next`, `roadmap`, `stats`, `burnup`, `org-board`, `org-status`)
and `[views.<name>]` tables in `myco.toml`
([spec fields](../reference/config.md)).

## Define one

```toml
# myco.toml
[views.hot]
filter = { pri_min = "high", status = ["open", "in-progress"] }
group_by = "owner"
sort = "-created"
format = "list"
```

```sh
myco view hot          # resolves the named view
myco view              # lists every defined view + origin — or renders
                       # `default_view` from config when one is set
myco view --status open --pri-min high   # ad-hoc spec from flags
myco view hot --limit 3                  # flags patch the named spec
```

Every read is also `--json`: `{view, spec, groups, interrupted}`.

## Views are the read surface

`myco view <name>` is *the* lens — there are no per-view verbs.
`view board` renders the kanban (Backlog | Open | Claimed | In
Progress | Blocked lanes); `view epic` and `view milestone` group
by that metadata, one column per group with `ungrouped` trailing.
`view next` is the "what do I pick up" preset — one open task or
issue, top of the default sort. `view org-board`/`org-status` are
the index-sourced org presets (see
[org views](org-views.md)).
The filter flags scope any view — `myco view board --epic v2` is
one epic's lanes.

## Analytics emits — roadmap, stats, burnup

Three formats ride the same `filter → group → sort` pipeline but
render aggregates instead of rows:

- `format = "timeline"` (the `roadmap` preset) — epic → milestone
  → cycle lanes with `due:` dates positioning items.
- `format = "stats"` (the `stats` preset) — count tallies per
  dimension (status, priority, owner, epic, milestone, cycle,
  labels).
- `format = "burnup"` (the `burnup` preset) — cumulative done-over-
  time vs total scope, one series per declared cycle plus an
  uncycled series. It reads the write-once `done:` archive stamp
  ([file spec](../explanation/file-spec.md)); pre-stamp archives
  stay valid but don't invent a completion date.

```toml
[views.velocity]
format = "stats"
filter = { status = ["done"] }

[views.next-cycle]
format = "burnup"
filter = { cycle = "sprint-13" }
```

All three emit structured JSON under `--json` (`{lanes}` /
`{stats}` / `{series}`) — the shape pavo consumes — and a terse
ASCII render on a TTY. Repo and index sources produce the same
payload.

## Iteration windows — `[[cycles]]`

Cycles are *scheduling config*, not ledger memory: `myco.toml`
declares named windows and item lines just carry `cycle:<name>`.

```toml
[[cycles]]
name = "sprint-13"
start = "2026-03-02"
end = "2026-03-15"        # last day, inclusive
```

- `end` must be after `start`; names must be unique and match
  `[A-Za-z0-9_.-]+` — invalid tables are a hard config error.
- Repo `[[cycles]]` entries shadow user-config entries by name
  (same layering as `[[kinds]]`). Rollover is manual: declare the
  next window when you start it.
- An item may carry any `cycle:` name — undeclared names are
  legal data. `myco check` warns (`unknown-cycle`) when a live
  item references a name no layer declares.
- `group_by = "cycle"` buckets views by cycle (uncycled items
  land in `ungrouped`), and the analytics emits read the table
  for burnup series and timeline lanes.

A config view shadowing a built-in name redefines `myco view <name>`
itself:

```toml
[views.board]
filter = { kind = ["issue"] } # view board is now issues-only
```

A shadow warns on stderr (`view "board" shadows the built-in
preset`) and `myco check` lints the repo layer's specs. The lane
chrome applies only to a status-grouped board spec: a shadowed
spec with a non-status `group_by` renders generic columns headed
by the group labels, and `--json` emits the `{view, spec, groups,
interrupted}` payload instead of the five-lane map. Specs that
admit non-lane buckets (e.g. `done`) surface them as extra
columns/keys — nothing matched is silently dropped.

Layering is repo &lt; user &lt; flags: a `[views]` table merges
across config layers; scalar fields stay first-match.

## Over the wire

`myco serve` exposes `view` (`{name?, spec?}` — `spec` is the
same per-field patch) alongside the rest of the verb surface;
`myco batch` reaches all of it one-shot:

```sh
printf '{"id":1,"method":"view","params":{"name":"hot"}}\n' | myco batch
```

## Verbs on top of views — `[verbs]`

`[verbs.<name>]` declares a verb of your own (P-007): `myco <name>`
then works like a builtin on every front end — CLI,
`myco batch`, and `myco serve` (it joins `initialize`'s method
list). Two kinds, both read-only:

```toml
# view-emit — a ViewSpec baked to one emit shape
[verbs.hot]
emit = "first"                     # table | json | ids | first
filter = { pri_min = "high", status = ["open"] }
sort = ["pri", "id"]

# alias — argv baked for a read-only builtin
[verbs.standup]
target = "view"
args = ["open"]
```

`myco hot` prints the top open high-priority item (`first` is
`next`-shaped but can't claim); `myco standup` is `myco view
open` with extra tokens appended (`myco standup --limit 3`).
`ids` prints one id per line — pipeable. `--json` works on every
kind.

Load-time rules: a verb name colliding with a builtin warns and
the builtin wins; an alias to a mutating or missing target warns
and is skipped; a view-emit spec goes through the same
`ViewSpec` validation as `[views]` — `myco check` lints the repo
layer's `[verbs]` (`verb-invalid` findings). Repo verbs merge
over user verbs on the same name (repo is shared surface);
verbs are named with the bare-word space only — a config verb
can't shadow inside a group.

Composites and hiding builtins are deferred — see
`proposal-0007`.
