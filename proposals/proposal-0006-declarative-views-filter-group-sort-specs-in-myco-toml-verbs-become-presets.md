+++
id = "P-006"
title = "declarative views — filter/group/sort specs in myco.toml; verbs become presets"
status = "open"
created = "2026-10-31"
priority = "medium"
labels = ["agent-dx"]
+++

## Context

Every read verb hardcodes its own slice of the ledger: `board`
fixes five status columns + `pri_then_id`; `status` fixes the
open-list digest; `epic`/`milestone` fix a single grouping axis.
"Which high-pri issues are unclaimed?" means picking the nearest
verb and filtering by eye. The P-004 batch work makes reads
cheaper; this makes them *shaped* — today's answer to "is the
issue list a priority queue?" is "sort of, if you read the whole
board."

Meanwhile `myco-config-toml` already resolves a layered config —
explicit path → `$MYCO_CONFIG` → `./myco.toml` →
`~/.config/myco/` — and views are the natural payload:
repo-committed presets the team shares, personal presets in user
config. Same model as taskwarrior reports / jj revset aliases.

## Plan

1. **View spec, pure in `myco-core`** — `filter → group → sort
   → format` as data. Filter is a *struct* (`kind`, `status[]`,
   `pri_min`/`pri_max`, `label[]`, `owner`, `epic`, `milestone`),
   not a query-language string. Sort is a named key list;
   format names an existing renderer (`list`/`board`/…).
   Evaluation is a pure fn over `&[Item]` — golden-testable,
   no I/O.

2. **`myco view <name>`** runs a named spec; `--list` shows
   defined views. Ad-hoc flags (`--kind`, `--pri-min`,
   `--status`, `--label`, `--group-by`, `--sort`) map onto the
   same filter struct — one evaluation path, no second grammar.

3. **Built-in verbs become presets** — `board`, `epic`,
   `milestone`, and `status`'s open-list re-expressed as
   built-in view definitions; same output, one engine. Config
   may override a built-in name to tune repo defaults (warn on
   shadow).

   ```toml
   [views.hot]
   filter = { kind = "issue", status = ["open","claimed","in-progress"], pri_min = "high" }
   group_by = "epic"
   sort = ["pri","id"]
   format = "board"
   ```

4. **TUI** — `myco ui --view hot` parameterizes the item
   source; the TUI already renders item lists.

## Non-goals

- **String filter DSL** (`pri>=high AND label:bug`) — a compat
  surface forever; structured fields carry the same power.
  Revisit only if conjunctions provably can't express a need.
- **Aggregation verbs** — `report` (temporal rollup) and
  `org status` (cross-repo join) aren't filter+sort; they stay
  specialized.
- **`status --prompt`** — an agent digest renderer, not a
  view; stays fixed.
- **Free-form templating** (`--format "{{id}}: {{title}}"`) —
  renderers stay named and deterministic; minimal-diffs ethos.
- **Ledger-versioned views under `project/`** — audit-trailed
  presets are plausible but YAGNI; config layering already
  covers shared vs personal.

## Order

Core spec + evaluation first (pure, goldens), then `myco view`
with ad-hoc flags, then the built-in-as-preset refactor, TUI
last. Issues to be filed on acceptance.
