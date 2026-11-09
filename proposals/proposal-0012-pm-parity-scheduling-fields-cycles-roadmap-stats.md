+++
id = "P-012"
title = "PM parity: scheduling fields, cycles, roadmap + stats"
status = "open"
created = "2026-11-09"
priority = "medium"
+++

# PM parity: scheduling fields, cycles, roadmap + stats

Status: **draft** · Created: 2026-11-09

## Goal & Overview

GitHub Projects' planning surface has five capabilities myco lacks:
due dates, estimates, cycles/iterations, a roadmap/timeline view,
and stats/charts. This proposal adds them — as additive, optional
item metadata and view emits — without breaking myco's constraints:
markdown canonical, agent-first (`--json` everywhere), no daemon,
dependency-light.

The design principle: **fields are data, scheduling is config,
views are emits.** Items gain three optional fields; cycles are
declared in `myco.toml` (config, not ledger — they're team
process, not work memory); roadmap and stats are new view emits
over the existing filter→group→sort pipeline, so they work on
repo ledgers *and* the org index for free.

## Requirements

- **R1 — item fields**: `due:YYYY-MM-DD`, `est:<n>`, `cycle:<name>`
  as line tokens and frontmatter keys; optional, additive,
  backward compatible with existing ledgers.
- **R2 — cycles in config**: `[[cycles]]` tables in `myco.toml`
  (name, start, end); `myco check` warns on items naming an
  unknown cycle (warning severity, per ADR-0002 — never an error).
- **R3 — roadmap emit**: `myco view <name>` with a timeline/roadmap
  emit renders lanes (epic → milestone → cycle) with items placed
  by `due`; JSON emit is structured for pavo/agents.
- **R4 — stats emit**: counts by status / priority / label /
  owner / cycle / epic — ASCII bars on a tty, JSON for machines.
- **R5 — surfaces**: `item set`/`todo add`/`backlog add`/kind `new`
  verbs gain the flags; view filter/group/sort axes accept the new
  fields; TUI preview shows them; index snapshots carry them.
- **R6 — no daemon**: cycle rollover is manual (config edit +
  `myco check`); nothing schedules itself.

## Technical Design

### Item model (`myco-core`)

```rust
pub due: Option<time::Date>,        // serde via crate::date::opt
pub estimate: Option<u32>,          // points; bounded (<= 1000)
pub cycle: Option<String>,          // [A-Za-z0-9_.-]+ like epic/milestone
```

Line grammar (TODO.md/BACKLOG.md and DONE.md archive lines):
`due:2026-12-01`, `est:3`, `cycle:sprint-12` — terse like `ms:`/`epic:`.
Frontmatter uses full names (`due`, `estimate`, `cycle`), matching
`pri:` ↔ `priority`.

`estimate` semantics: relative size in points, not hours — document
as such; teams define their own scale. `due` is a calendar date,
timezone-free (`time::Date` already in core).

### Config (`myco.toml`)

```toml
[[cycles]]
name  = "sprint-12"
start = "2026-11-10"
end   = "2026-11-21"
```

Validation: unique names, `end > start`, ISO dates — config errors
are hard (config is code, not data). Item `cycle:` may name a
configured cycle or be free-form; configured cycles additionally
enable "current cycle" computation and roadmap placement.

### Views

Filter/group/sort axes gain `due`, `estimate`, `cycle`. New emits:

- **`timeline`** — lanes ordered epic → milestone → cycle (flat
  fallback when ungrouped); items sorted by `due` then priority;
  ASCII columns per week for tty, structured JSON
  `{lane, items[{id, due, est, cycle}]}` for pavo.
- **`stats`** — grouped counts with ASCII bars on tty; the same
  eval pipeline yields JSON on `--json`.

Both work unchanged on `source = "index"` specs — org roadmaps
are the point.

### Verbs & surfaces

- `item set <id> --due 2026-12-01 --est 3 --cycle sprint-12`
  (`""` clears — same convention as `--epic`/`--milestone`).
- `todo add`/`backlog add`/file-kind `new` mirror the flags.
- TUI preview renders the fields (metadata section iterates known
  fields); `/` filter and view specs cover them automatically.
- Index: redb snapshot schema bump — old snapshots rescan once.
- `myco report` can gain a cycle dimension later; out of scope here.

### Phases — each independently shippable

1. **Fields**: `due`/`estimate`/`cycle` on `Item`, line tokens,
   frontmatter, `item set`/add flags, `myco schema` update,
   store-md goldens. Unblocks sorting/filtering immediately.
2. **Cycles config**: `[[cycles]]` parsing/validation, `check`
   warnings for unknown cycle names, group-by-cycle axis.
3. **Emits**: `timeline` + `stats` view emits (tty + JSON),
   index-source support, docs.
4. **Stretch**: `done:YYYY-MM-DD` stamp on archive → historical
   burnup; pavo roadmap widget. Deferred — needs its own proposal
   if pulled forward.

### Failure modes

- **Old binaries vs new tokens**: store-md strict parse rejects
  unknown tokens — a ledger using `due:` needs a current myco.
  Same class as every spec change; document minimum version in
  `migrate`/docs. No silent degradation — strictness is the spec.
- **Invalid values**: bad dates/estimates rejected at write time;
  `check` validates persisted files.
- **Cycle drift**: renamed/removed cycles → `check` warnings, items
  keep their (now unconfigured) cycle name — never data loss.
- **Index staleness**: snapshot version bump forces one rescan.
- **Timezones**: dates are calendar dates; "overdue" is computed
  against local today — documented, no tz math.

### Migration

Additive only — no data migration, no `migrate` changes. Golden
fixtures update for the new tokens. Ledgers without the fields are
untouched.

### Test plan

- core: token parse/emit roundtrip; date + estimate validation;
  cycle-name charset.
- store-md: goldens with new tokens; strict-parse rejection of
  malformed `due:`/`est:`.
- config-toml: `[[cycles]]` parse, duplicate/ordering validation.
- ops: `item set`/add flags incl. `""` clears; `check` warning on
  unknown cycle.
- view: filter/group/sort on the new axes; `timeline`/`stats`
  deterministic-output tests, index-source parity.
- tui: preview renders fields; index snapshot rescan on version
  bump.
- e2e per phase: proposal → plan → tasks.

## Open questions

- Token spellings: `est:` vs `pts:`; `cycle:` vs `iter:` —
  bikeshed at implementation, default to the shortest readable.
- Estimate unit: points vs t-shirt sizes — start with integer
  points, revisit if teams want buckets.
- Roadmap lane order: epic → milestone → cycle proposed; flat
  milestone-only ledgers should still render sanely.
- Should `stats` be a verb (`myco stats`) or only a view emit?
  Leaning emit — reuses the pipeline, JSON comes free; a verb can
  alias it later if needed.
