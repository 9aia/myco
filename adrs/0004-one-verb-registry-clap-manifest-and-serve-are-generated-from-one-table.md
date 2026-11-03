+++
id = "ADR-0004"
title = "One verb registry — clap, manifest and serve are generated from one table"
status = "open"
created = "2026-11-03"
priority = "medium"
labels = ["agent-dx"]
+++

Status: **accepted** · Created: 2026-11-02

## Context

P-007 opened with the observation that the verb surface was
triplicated — the clap tree (truth), a hand-maintained
`manifest().verbs` array, and a hand-maintained `serve::METHODS`
array — and had already drifted (serve lacked `epic`, `milestone`,
`index.*`, `org.*`, `absorb`, `init`, `migrate`, `commit`). Every
new verb was three edits and a parity bug waiting to happen.

P-007 shipped the fix and is closed. The decision outlived it.
"One registry, three front-ends" is not a task that completes; it
is a constraint every future verb must satisfy, and it is
invisible to anyone reading a proposal marked `done`.

## Decision

A single [`VERBS`] table in `crates/myco-cli/src/registry.rs` is the
only verb surface. It declares, per verb: name, group, about,
params (kind, wire key, value type, which front-ends see it),
handler. From it are generated:

- the clap command tree (builder mode, no derive),
- `manifest().verbs` and `manifest().implemented`,
- serve/batch dispatch (JSON-RPC methods share the CLI's param
  schema; one `Args` bag decodes both).

Consequences of "one table":

- **A verb cannot exist on one surface and not the others.** Adding
  a verb is one row; omitting a surface is a compile error, not a
  parity test.
- **Params are typed once.** `Val` (`Id`/`Status`/`Pri`/`Kind`/`Sort`/…)
  drives both the clap validator and the wire-side parse, so the
  CLI and the JSON-RPC surface cannot disagree about what a field
  means.
- **Config verbs are data, not rows.** `[verbs.*]` in `myco.toml`
  resolves per call through `myco_ops::verb` and executes via
  `cfgverb`; builtins always win a name collision (warned).
  Mutations are never config-declared — state machine, id
  allocation, locking and commit semantics are not data.
- **Builder-mode clap** is accepted: we lose derive-time checking
  in exchange for a surface that exists once.

## Consequences

- The registry is now the largest single module in the binary
  (~1.9k lines) and it is *supposed* to be: it is the specification
  of the product's surface.
- Prose that lists verbs (skill, docs, README) is generated or
  derived from `manifest`, so drift shows up as a diff rather than
  as a lie. `myco manifest` is the single source for "what does
  this binary do".
- Rejected: composite/multi-step verbs (ordering + failure
  semantics unsolved); config verbs shadowing builtins (the stable
  surface is part of the audit trail).
- Related: P-006 (views as data) makes *reads* configurable for the
  same reason — a fixed preset per question is a surface nobody
  can extend.
