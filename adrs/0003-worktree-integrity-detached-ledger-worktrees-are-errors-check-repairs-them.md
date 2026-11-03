+++
id = "ADR-0003"
title = "Worktree integrity — detached ledger worktrees are errors, check repairs them"
status = "done"
created = "2026-11-03"
priority = "high"
labels = ["git"]
+++

Status: **accepted** · Created: 2026-11-02

## Context

The design's load-bearing sentence is "the `project` branch
history is the audit trail". `check` validated the *ledger* (parses,
ids, counters, refs) and never looked at *git*. So the one
condition that silently voids the audit trail went unreported: a
`project/` worktree on a **detached HEAD**.

Measured on the six live 9aia ledgers: `arca/project` was detached,
16 auto-commits (`chore: done T-005`, `chore: handoff T-005`, …)
ahead of `origin/project`, with **no `refs/heads/project` at all** —
reachable only through that worktree's HEAD, one `git worktree
prune` + `gc` from gone, and invisible to `git log project`.

Nothing prevented recurrence. Discovery accepted any dir holding a
`TODO.md` (`discover::qualifies`), so a detached worktree still
qualified as a "plain-directory ledger"; and `commit_all` committed
wherever HEAD happened to point.

## Options

- **Detect only** — report a detached worktree, let a human fix it.
  Cheap; the failure keeps happening for anyone not reading check
  output (which, per ADR-0002, is exactly the population that
  ignores gates).
- **Refuse writes** — `ensure_committable` fails the verb. Safe,
  but a repo already detached becomes unworkable with no way out.
- **Report + refuse + repair** — the choice below.

## Decision

Three layers, in this order of authority:

1. **`check` reports worktree health** as git-side findings:
   `worktree-detached` (**error**, fixable), `worktree-branch`
   (warning: a worktree root checked out on a branch that isn't
   `project` — legacy in-tree layout, `myco migrate` territory),
   `worktree-unpushed` (warning: N commits on `project` that
   `origin/project` doesn't have — the trail is machine-local).
   Health is evaluated **only for a checkout that is its own
   worktree root**: a plain `project/` subdirectory rides the
   enclosing branch by design and must stay quiet.
2. **`check --fix` reattaches** — `git checkout -B project`
   (`myco_git::attach_branch`): create-or-reset at the current
   HEAD, so the detached commits *become* the branch tip rather
   than being abandoned, and any uncommitted writes made against
the detached HEAD are committed onto the branch. Git refuses when
   another worktree holds `project`; that is a human call.
3. **Every write path refuses a detached ledger** —
   `myco_ops::ensure_committable` at the top of `open_with` (the
   single choke point for mutating verbs), and
   `myco_git::commit_all` as defence in depth. The error names the
   repair (`run `myco check --fix``).

`check`, `commit`, `migrate` and `init` take the worktree lock
directly rather than through `open_with`, so the repair paths still
reach a ledger that every other verb now refuses.

## Consequences

- A ledger cannot be written into unreachability: either the write
  fails with an actionable message, or a branch records it.
- `check` is meaningful as a CI gate for the property the whole
  design rests on — the same class of invariant as an orphan
  handoff, which it already policed.
- Pushing stays out of `check`: `--fix` never touches the network,
  and `worktree-unpushed` is a warning because "local-only" is a
  legitimate mode (offline, no remote, pre-publication).
- Cost: one `git rev-parse` pair per `check` (`is_detached`), and
  the plain-dir fallback keeps working for repos that never
  adopted the worktree layout.
