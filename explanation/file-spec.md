# The ledger file spec

What `project/` contains, how items are written, and the state
machine they live under. `myco schema` emits all of this as
machine-readable JSON Schema; `myco check` enforces it.

## Layout

| path         | holds                                                               |
| ------------ | ------------------------------------------------------------------- |
| `meta.toml`  | `name` + `[next]` per-kind id counters (`t`, `i`, `p`, `adr`, `pl`) |
| `TODO.md`    | open-line tasks (`T-`), `##` sections, nested children              |
| `BACKLOG.md` | deferred items — same grammar, no checkbox                          |
| `DONE.md`    | task archive — same grammar, status `done` or `cancelled`          |
| `index.md`   | free-form human index — verbatim, never rewritten                   |
| `issues/`    | `issue-NNNN-<slug>.md` + `issues/archive/`                          |
| `proposals/` | `proposal-NNNN-<slug>.md` + `proposals/archive/`                    |
| `plans/`     | `pl-NNNN-<slug>.md` + `plans/archive/`                              |
| `adrs/`      | `NNNN-<slug>.md` + `adrs/archive/`                                  |
| `handoffs/`  | `<ID>.md` append-only + `handoffs/archive/`                         |

## Item ids

`<prefix>-<number>`, zero-padded per kind: `T-001` tasks,
`I-001` issues, `P-001` proposals, `PL-001` plans, `ADR-0001`
ADRs. One registry, per-kind counters in `meta.toml`'s `[next]`
table — ids are never reused. Tasks live as lines; the four
file-backed kinds (`I`, `P`, `PL`, `ADR`) get a markdown file
each, filenames padded to four digits.

## Line items — TODO.md / BACKLOG.md / DONE.md

```markdown
# TODO — <repo>

## <section>

- [ ] T-012 <title> · status:in-progress · pri:high · owner:devin · created:2026-10-29 · #label · blocks:I-001 · links:PL-001,P-013 · epic:v2 · ms:1.0 · due:2026-12-01 · est:3 · cycle:sprint-12 · done:2026-12-19 · note:<text>
  - [ ] T-013 a sub-task — children are the same grammar, indented 2 spaces/level
```

- `BACKLOG.md` lines are the same shape minus `[ ]`, default
  status `deferred`; `TODO.md` defaults to `open`. `DONE.md` is
  the task archive — same line grammar, status `done`
  (the default, token dropped) or `cancelled` (token kept, so
  the archive records *why* it left), read by `myco done list`
  and still resolvable via `myco item show`. The archive isn't
  a one-way door: `item set --status open|deferred` reopens an
  item (subtree included — it lands top-level in `TODO.md` or
  `BACKLOG.md`) with a `reopened` handoff as the audit trail,
  and `myco done remove` deletes an archived subtree outright.
  Items stamped `status:done` also carry a write-once `done:`
  archive stamp — the local date the item reached `done`, written
  by the archive path (`todo done`, `issue resolve`, file-kind
  moves, …) and never rewritten on re-save. `cancelled` items
  stay unstamped. Reopening clears the stamp; a later re-archive
  stamps the new date. Pre-stamp archives simply have no `done:` —
  `myco view burnup` counts them only if another signal dates them.
- `done` moves the line (subtree included) to `DONE.md` —
  completed work stays visible instead of vanishing;
  `cancel` does the same but lands as `status:cancelled` —
  "never doing this" is recorded honestly instead of polluting
  the backlog or posing as done; `defer` moves it to
  `BACKLOG.md`; `promote` moves it back. `cancel` also works
  straight from `BACKLOG.md` — no promote needed.
- `done`/`cancel`/`defer` on a parent act on the whole subtree;
  on a child, just that node.
- Token order is canonical (`status`, `pri`, `owner`, `created`,
  `#label`, `blocks`, `links`, `epic`, `ms`, `due`, `est`, `cycle`,
  `done`, `note`) so emit produces minimal diffs. `ms` is the milestone
  token — kept short. `links:` declares kind-agnostic associations
  — the plan, proposal, or ADR an item belongs to (`myco item set
  --links` / `--unlink`). `due` is a `YYYY-MM-DD` date, `est` a
  point estimate (0–1000, `--est`/`--estimate`), `cycle` an
  iteration name, `done` the archive stamp (above) — the
  scheduling trio (`due`/`estimate`/`cycle` in file frontmatter,
  `done` alongside them once archived).
- Patterns: actors `[A-Za-z0-9._:-]+`, labels `[A-Za-z0-9_-]+`,
  epic/ms/cycle names `[A-Za-z0-9_.-]+`, priority
  `low|med|high|crit`.

## File items — issues/ proposals/ plans/ adrs/

`+++` TOML frontmatter + free-form markdown body:

```markdown
+++
id = "P-001"
title = "myco v2 — agent-first project management, in Rust"
status = "open"
created = "2026-10-29"
priority = "med"
labels = ["design"]
blocks = ["T-008"]
links = ["PL-001", "P-013"]
epic = "v2"
milestone = "1.0"
due = "2026-12-01"
estimate = 3
cycle = "sprint-12"
done = "2026-12-19"
+++

# Title

Body — templates seed Context/Decision/Consequences-shaped
sections per kind.
```

Frontmatter keys: `id`, `title`, `status`, `created`, `owner`,
`priority`, `labels`, `blocks`, `links`, `note`, `superseded_by`,
`supersedes`, `epic`, `milestone`, `due`, `estimate`, `cycle`,
`done`.
`priority` spells the `pri:`
token (`low|med|high|crit`) — the serde full names `medium` /
`critical` still read but re-emit as tokens on any write. `due`
is a `YYYY-MM-DD` string (a bare TOML date reads the same and
re-emits quoted), `estimate` a 0–1000 point count, `cycle` a
name — unknown keys are ignored on read (forward-compat) and
dropped on re-emit.
Archiving moves the file to `<dir>/archive/` — `myco item show`
still finds archived items.

## State machine

Seven statuses; `item set --status` and kind verbs walk it one
legal step at a time:

| from          | can go to                                            |
| ------------- | ---------------------------------------------------- |
| `open`        | `claimed`, `in-progress`, `deferred`, `cancelled`    |
| `claimed`     | `in-progress`, `open`, `deferred`, `cancelled`       |
| `in-progress` | `blocked`, `done`, `deferred`, `open`, `cancelled`   |
| `blocked`     | `in-progress`, `open`, `deferred`, `cancelled`       |
| `deferred`    | `open`, `cancelled`                                  |
| `done`        | `open`, `deferred` (reopen — leaves `DONE.md`)       |
| `cancelled`   | `open`, `deferred` (same)                            |

Proposal words alias the machine: `draft` = `open`, `accepted` =
`done`, `rejected` = `deferred` — the transition still walks the
graph (`accepted` passes `open → in-progress → done`). ADR status
`open` reads as "proposed"; `adr supersede` closes the old file
with `superseded_by:` and stamps the replacement `supersedes:`.

## Handoffs

`handoffs/<ID>.md` — one file per item, uniform across kinds:

```markdown
+++
item = "T-012"
+++

# Handoffs — T-012

## 2026-10-29 · devin · session:cli:abc123

Done: parser. Next: emitter. Open: archive semantics.
```

Append-only: every `myco handoff` is one `##` entry + one commit;
entries are never rewritten. When an item's file goes away, its
handoff file is orphaned — `myco check` reports it and
`check --fix` moves it to `handoffs/archive/`, where `resume`
still finds it.
