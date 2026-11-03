+++
id = "ADR-0002"
title = "ID durability — declared refs are errors, prose refs are warnings"
status = "done"
created = "2026-11-03"
priority = "high"
labels = ["ledger"]
+++

Status: **accepted** · Created: 2026-11-03

## Context

Two rules collide.

1. **Doc-storage items are deleted when they finish.**
   `myco-store-md` removes `[x]`/`status:done` lines from
   `TODO.md`/`BACKLOG.md` (`todo.rs`: "done tasks are removed from
   TODO.md"); git holds the history. That is what keeps the ledger
   minimal-diff and free of tombstones.
2. **The ledger is the durable record of intent.** Handoffs, issue
   bodies, proposals, ADRs and `index.md` all cite ids in prose.

Put together, completing a task *guarantees* that every durable
artifact naming it points at an id no longer in the ledger. The
`check` verb calls those `dangling-ref`, `--fix` cannot repair them
(it cannot know whether the author meant a renumber, another repo,
or history), and it reported them as errors — so the documented CI
gate (`myco check` in pipelines) was red on 5 of 6 live ledgers,
permanently and by construction. A gate that always fails is a gate
everyone ignores.

The same measurement found the second failure class:
`orphan-handoff` (a `handoffs/<ID>.md` whose item is gone) — fixable
by archiving, but only *after* the fact.

## Options

- **Tombstones** — keep done doc lines in a `## Done` section or a
  `done.md`. Durability restored, minimal diffs lost; the ledger
  grows forever in the files agents read every turn.
- **Authorship discipline** — ban `T-NNN` from prose, cite only
  stable kinds (`I-`, `P-`, `ADR-`, `PL-`). Cheap, but it forbids
  the most natural sentence in a handoff (`T-NNN done: …`)
  and the rule is invisible at write time.
- **Severity split** — unknown ids in *prose* are warnings;
  unknown ids in *declared* positions (frontmatter `blocks:`,
  `supersedes:`, handoff filenames) stay errors.

## Decision

**Severity split, both halves.**

- `check` findings carry a severity. `error` fails the gate;
  `warning` is reported (in human and `--json` output, with a
  `warnings` count) and costs nothing.
- **Declared references are errors** — `blocks:`, `superseded_by:`,
  a `handoffs/<ID>.md` filename, a title/H1 id claim. These are
  machine-read edges (P-005) and an unresolvable one is a real
  defect. `orphan-handoff` stays an error *and* stays fixable
  (archive it).
- **Prose references are warnings** — a bare `PREFIX-NNNN` in a
  body. Doc items are deleted when done, so the reference is
  correct history, not a defect.
- `--json` gains `severity` per finding and a top-level `warnings`
  count; `ok` now means "no errors", not "no findings".
- The human summary reads `N error(s), M warning(s)`, and a green
  run with warnings says `check: clean (M warning(s))` rather than
  pretending nothing was seen.

## Consequences

- The gate can be green honestly, so it can be *enforced* — which
  is the point of having a gate.
- Knowledge is not lost: a stale ref still prints, still carries
  file:line:col, and still shows up in `serve`'s `check` payload.
- We keep minimal diffs (rule 1) and cite freely in prose (rule 2).
  The cost is that a broken *cross-item link* in prose needs a
  human; nothing machine-readable depends on it.
- Follows from this: **cite ids that outlive their subject are the
  author's problem, not the validator's.** If a warning on every
  handoff becomes noise, the answer is a done-log (option 1), and
  that is a new ADR, not a severity flip.
- Gap, recorded not fixed: `check` flags a dangling `blocks:`
  (`dangling-blocks`) but not yet a dangling `supersedes:`/
  `superseded_by:` — `refs::extract` puts both on the index
  (P-005), the validator only calls one. Under this ADR that is
  an error-class bug owed to `check`, not a reopened question.
- `myco check --fix` now also repairs *worktree health* (see
  ADR-0003): a detached `project/` worktree is the one condition
  under which the audit trail is not merely incomplete but
  unreachable, so it is always an error.
