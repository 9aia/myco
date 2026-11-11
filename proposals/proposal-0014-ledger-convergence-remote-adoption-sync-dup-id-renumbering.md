+++
id = "P-014"
title = "ledger convergence: remote adoption, sync, dup-id renumbering"
status = "done"
created = "2026-11-11"
priority = "med"
+++

# ledger convergence: remote adoption, sync, dup-id renumbering

Status: **done** · Created: 2026-11-11 · Implemented: 8ad39b0f

## Goal & Overview

Two clones committing to the `project` branch forked the audit
trail into unrelated histories (the tarsi incident), and
concurrent writers minted the same ids on stale counters
(ktbhealth needed a hand-rolled `renumber colliding ids` commit).
This proposal records the shipped design: prevent the fork,
gate mutations on freshness, and heal duplicate ids
deterministically at sync time.

## Requirements

- `init`/`ensure()` must never create a second orphan root when
  `origin/project` exists — fetch first, adopt the remote tip.
- Mutating verbs must refuse while the ledger is behind the
  last-fetched remote — every commit would deepen divergence.
- `myco sync` is the convergence verb: fetch, fast-forward or
  rebase, then heal merge damage; runnable repeatedly (a
  hand-resolved rebase still needs the heal pass).
- Duplicate ids resolve remote-wins: the pushed item keeps its
  id, the local/rebased item moves to a fresh id, and every
  reference myco owns inside `project/` is rewritten.
- Ambiguity is reported, never guessed.

## Technical Design

- `init::ensure` fetches (`myco_git::sync`, best-effort) and
  checks `refs/remotes/origin/project` before branching:
  remote exists → `git branch --track project origin/project`;
  neither side → orphan root as before. `InitOutcome.
  adopted_remote` surfaces the adoption in output/JSON.
- `myco_ops::ops::ensure_synced` runs in the shared mutation
  open path (`items::open_with*`) and `myco commit`: behind
  `origin/<branch>` → refuse with "run `myco sync`". Uses the
  last-fetched ref (no network); untracked branches pass.
- `myco sync` (`cli/src/sync.rs`): fetch (offline-tolerant),
  `merge --ff-only` when local has nothing to replay,
  `rebase --autostash` when diverged. Conflicts stop mid-flight
  with the unmerged file list; `--abort` bails. The heal tail
  always runs — `check::renumber_duplicates` then `check --fix`
  (counters, emit drift, refs) — including on up-to-date syncs.
- `check::renumber_duplicates`: for each duplicated id, items
  reachable from `origin/<branch>` keep their ids; the rest are
  renumbered to fresh ids via the existing title-based
  renumber machinery, rewriting item files, `TODO.md`/
  `BACKLOG.md` lines, handoffs, and `blocks:`/`links:`/
  `supersedes:` refs. Dup existing on *both* sides → reported
  for manual resolution.
- `myco_git` gained `commits_behind`, `merge_ff`, `rebase`,
  `rebase_abort`, `rebase_in_progress` (state dirs
  `rebase-merge`/`rebase-apply`/`MERGE_HEAD` — `REBASE_HEAD`
  persists after a finished rebase), `unmerged_files`.
- `check` warns `worktree-behind` naming `myco sync` as the fix;
  `worktree-unpushed` already covered the other direction.

## Known limits

- References *outside* `project/` cannot be rewritten: commit
  trailers (`Item: T-012`), sturn claims, chat history, external
  trackers. `sync` prints the renumber map (`T-002 → T-004`) as
  the audit trail for those.
- Textual conflicts on shared docs (`TODO.md` appends) remain
  manual — sync cannot tell whether two lines with the same id
  are the same work or different work.
- Offline writers can still mint colliding ids; the loser heals
  on their next `sync`. The guard shrinks the window to
  "whoever synced last".

## Open questions

- Should `myco serve`/driver expose `sync` on the wire (today
  CLI-only, like `init`/`migrate`)? Rebase conflicts want a tty.
- Auto-push after sync? Currently push stays manual — the
  unpushed warning is the reminder.
