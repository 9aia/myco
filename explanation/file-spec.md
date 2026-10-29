---
url: /myco/explanation/file-spec.md
---
# The ledger file spec

What `project/` contains, how items are written, and the state
machine they live under. `myco schema` emits all of this as
machine-readable JSON Schema; `myco check` enforces it.

## Layout

| path | holds |
|---|---|
| `meta.toml` | `name` + `[next]` per-kind id counters (`t`, `i`, `p`, `adr`, `pl`) |
| `TODO.md` | open-line tasks (`T-`), `##` sections, nested children |
| `BACKLOG.md` | deferred items — same grammar, no checkbox |
| `index.md` | free-form human index — verbatim, never rewritten |
| `issues/` | `issue-NNNN-<slug>.md` + `issues/archive/` |
| `proposals/` | `proposal-NNNN-<slug>.md` + `proposals/archive/` |
| `plans/` | `pl-NNNN-<slug>.md` + `plans/archive/` |
| `adrs/` | `NNNN-<slug>.md` + `adrs/archive/` |
| `handoffs/` | `<ID>.md` append-only + `handoffs/archive/` |

## Item ids

`<prefix>-<number>`, zero-padded per kind: `T-001` tasks,
`I-001` issues, `P-001` proposals, `PL-001` plans, `ADR-0001`
ADRs. One registry, per-kind counters in `meta.toml`'s `[next]`
table — ids are never reused. Tasks live as lines; the four
file-backed kinds (`I`, `P`, `PL`, `ADR`) get a markdown file
each, filenames padded to four digits.

## Line items — TODO.md / BACKLOG.md

```markdown
# TODO — <repo>

## <section>

- [ ] T-012 <title> · status:in-progress · pri:high · owner:devin · created:2026-10-29 · #label · blocks:I-001 · epic:v2 · ms:1.0 · note:<text>
  - [ ] T-013 a sub-task — children are the same grammar, indented 2 spaces/level
```

* `BACKLOG.md` lines are the same shape minus `[ ]`, default
  status `deferred`; `TODO.md` defaults to `open`.
* `done` removes the line (history keeps it); `defer` moves it to
  `BACKLOG.md`; `promote` moves it back.
* `done`/`defer` on a parent act on the whole subtree; on a
  child, just that node.
* Token order is canonical (`status`, `pri`, `owner`, `created`,
  `#label`, `blocks`, `epic`, `ms`, `note`) so emit produces
  minimal diffs. `ms` is the milestone token — kept short.
* Patterns: actors `[A-Za-z0-9._:-]+`, labels `[A-Za-z0-9_-]+`,
  epic/ms names `[A-Za-z0-9_.-]+`, priority `low|med|high|crit`.

## File items — issues/ proposals/ plans/ adrs/

`+++` TOML frontmatter + free-form markdown body:

```markdown
+++
id = "P-001"
title = "myco v2 — agent-first project management, in Rust"
status = "open"
created = "2026-10-29"
priority = "medium"
labels = ["design"]
blocks = ["T-008"]
epic = "v2"
milestone = "1.0"
+++

# Title

Body — templates seed Context/Decision/Consequences-shaped
sections per kind.
```

Frontmatter keys: `id`, `title`, `status`, `created`, `owner`,
`priority`, `labels`, `blocks`, `note`, `superseded_by`,
`supersedes`, `epic`, `milestone`. Archiving moves the file to
`<dir>/archive/` — `myco item show` still finds archived items.

## State machine

Six statuses; `item set --status` and kind verbs walk it one
legal step at a time:

| from | can go to |
|---|---|
| `open` | `claimed`, `in-progress`, `deferred` |
| `claimed` | `in-progress`, `open`, `deferred` |
| `in-progress` | `blocked`, `done`, `deferred` |
| `blocked` | `in-progress`, `open`, `deferred` |
| `deferred` | `open` |
| `done` | — (terminal) |

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
