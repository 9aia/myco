+++
id = "P-009"
title = "the engine underneath — myco as records+views+mutations engine, PM as shipped prelude"
status = "open"
created = "2026-11-03"
owner = "devin"
priority = "high"
labels = ["architecture"]
+++

## Context

Something emerged this arc that nobody designed: every feature
pulled the same direction, and the seams line up into an engine.

- `ViewSpec` = `{source, filter, group_by, sort, limit, format}` —
  the read pipeline is `universe → eval → emit`
- `source` made *universes* data (repo store | index union)
- `format` made *emits* data (list | board | md | json | repos)
- `[[kinds]]` made the *item vocabulary* data (prelude, not enum)
- `[views]`/`[verbs]` made the *surface* data (shadow, alias,
  view-emit)
- The registry made *dispatch* data (one table → clap/manifest/wire)

What remains hardcoded is the schema: `Item`'s fields, the state
machine, the file spec — the thing myco must *own* to guarantee
integrity. Everything else is configuration.

## Thesis

**myco is a records+views+mutations engine; PM is its shipped
prelude.** `myco init` already seeds the prelude (`[[kinds]]` =
task/issue/proposal/adr/plan, builtins views, registry verbs) —
a user editing config isn't "customizing a PM tool," they're
programming an engine whose default program happens to be PM.

## Implications

- **Boundary discipline**: engine work = sources, axes, formats,
  kinds, verb declarations, eval/emit machinery. Prelude work =
  which kinds/views/verbs ship seeded. Every feature gets asked
  "engine or prelude?" — engine answers generalize, prelude
  answers configure.
- **What myco owns** (non-negotiable, the product's moat): the
  markdown file spec, atomic commit-per-mutation on `project`,
  the state machine semantics, ID registry. What users own:
  everything you can *look* at or *declare*.
- **Sources grow**: repo/index today; sturn signals, peregrin
  state, arbitrary ledgers later — `source = "x"` is the port.
- **Formats grow**: `repos` proved the universe itself is a
  render target; next candidates are emit shapes (table/fields).
- **Identity**: docs/naming should eventually reflect it —
  engine layer vs prelude layer — not rename now, but write new
  docs in that framing.

## Non-goals

- NOT going fully generic: `Item` keeps fixed semantics. A
  schema-DSL (declared fields/axes/formatters) turns this into
  jq-over-markdown — a different product, and a worse one.
- No crate renames or split as part of this proposal.
- Mutations stay code — the prelude configures *what* exists,
  not *how* writes behave.

## Direction

1. Ratify the framing (this doc) — future items judged
   engine-vs-prelude.
2. Docs: architecture page gains the engine/prelude diagram.
3. New sources/formats land as engine features; new kinds/
   views/verbs ship as prelude entries.
