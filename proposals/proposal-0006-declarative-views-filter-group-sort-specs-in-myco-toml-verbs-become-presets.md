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

Meanwhile `myco-config-toml` already finds config — explicit
path → `$MYCO_CONFIG` → `./myco.toml` → `~/.config/myco/` —
though today it's first-match, not layered: a repo `myco.toml`
shadows user config entirely. Views motivate the first merged
section: `[views]` tables union across layers, so repo-committed
presets are shared by the team while personal presets live in
user config. Same model as taskwarrior reports / jj revset
aliases.

## Plan

1. **View spec, pure in `myco-core`** — `filter → group → sort
   → limit → format` as data:

   ```toml
   [views.hot]
   filter = { kind = ["issue"], status = ["open","claimed","in-progress"], pri_min = "high", claimed = false }
   group_by = "epic"
   sort = ["pri","id"]
   limit = 10
   format = "board"
   ```

   - *Filter* is a struct, not a query-string DSL: `kind[]`,
     `status[]`, `pri_min`/`pri_max`, `label[]`, `owner[]`,
     `claimed`, `epic`, `milestone`, `superseded`. OR within a
     list, AND across fields. `pri_min`/`pri_max` are inclusive
     severity bounds (crit > high > med > low). `claimed = false`
     means owner absent — it answers the motivating question;
     `claimed` + `owner` together is a validation error. Kind
     values are serde names (`task|issue|proposal|adr|plan`),
     not id prefixes.
   - *Group* is one axis: `status|priority|kind|owner|epic|
     milestone`. Items missing the axis land in a trailing
     `ungrouped` bucket.
   - *Sort* is a named key list over `pri|id|created|status|
     kind|owner|title`; a `-` prefix flips the key's default
     direction (`pri` is crit-first, matching `pri_then_id`;
     absent fields like `created` sort last).
   - *Limit* truncates post-sort — `limit = 1` is `myco next`
     (I-020) as a preset.
   - *Format* names a renderer and `group_by` feeds its axis:
     `list` renders flat (headed groups when `group_by` is set);
     `board` renders columns, axis defaulting to `status`.

   Filters see top-level items only — children render with
   their parent. The universe is every item in the ledger:
   TODO.md, BACKLOG.md, the kind dirs; no implicit status
   restriction. Evaluation is a pure fn over `&[Item]` —
   golden-testable, no I/O. The field set is additive:
   `created` bounds, note/handoff predicates, dep-blocked
   (needs P-005) land later without a grammar change.

2. **`myco view <name>`** runs a named spec; `myco view --list`
   enumerates defined views. Ad-hoc flags (`--kind`, `--status`,
   `--pri-min`, `--pri-max`, `--label`, `--owner`, `--unclaimed`,
   `--group-by`, `--sort`, `--limit`, `--format`) build the same
   struct — one evaluation path, no second grammar. A named view
   plus flags merges: flags override set fields, unset fields
   are preserved. `--json` emits the resolved spec plus matched
   items. Unknown view names, group axes, and sort keys are
   validation errors — `myco check` lints `[views]` in
   myco.toml.

3. **Config merge; built-ins become presets** — `Config` gains
   `views: BTreeMap<String, ViewSpec>`; `myco-config-toml` loads
   every reachable layer instead of stopping at the first.
   Precedence, low→high: built-ins < repo (`./myco.toml`, or the
   explicit/`$MYCO_CONFIG` file when set) < user
   (`$XDG_CONFIG_HOME/myco/` preferred over `~/.config/myco/`)
   < CLI flags. Higher layers win name collisions; shadowing a
   *built-in* name warns. `board`, `epic`, `milestone`, and
   `status`'s open-list are re-expressed as built-in
   definitions — same output, one engine.

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
- **Mutations through views** — `view` is read-only; multi-id
  `item set` (P-004) stays the write path.
- **Ledger-versioned views under `project/`** — audit-trailed
  presets are plausible but YAGNI; config layering already
  covers shared vs personal.

## Order

Core spec + evaluation first (pure, goldens), then config merge
+ `myco view` with ad-hoc flags, then the built-in-as-preset
refactor, TUI last. Issues to be filed on acceptance.
