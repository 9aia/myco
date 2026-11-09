+++
id = "P-011"
title = "multi-level undo/redo journal"
status = "in-progress"
created = "2026-11-09"
priority = "medium"
+++

## Problem

`myco undo`/`redo` are single-step over the git tip: undo = `reset --hard HEAD~1`, redo = `reset --hard HEAD@{1}` gated by the reflog message `reset: moving to HEAD~1`. Repeated undos walk back fine, but redo restores only the *most recent* undo — there is no redo stack, no history view, and the reflog-message sniffing is fragile coupling.

## Proposal: explicit undo journal

Track position with a small state file **outside the worktree** — `<git-common-dir>/myco-history.json` (never committed to `project/`; it's meta-state, not ledger content). The ledger's linear commit chain remains the "past"; the file only records the "future" — commits that were undone and are still redoable.

```json
{"version": 1, "future": [{"target": "<sha>", "base": "<sha>", "subject": "...", "date": "..."}]}
```

- `target` — commit to restore on redo.
- `base` — HEAD position that entry was undone *from*; validity anchor.

### Semantics (classic editor model)

- **undo** — always `reset --hard HEAD~1` (git history is the past; no validity needed). After a successful reset, push `{target: oldHEAD, base: newHEAD}` onto `future` and clear nothing else. First, drop `future` if it's stale (see below).
- **redo** — valid iff `future` non-empty **and** `HEAD == future.top.base`. Then `reset --hard future.top.target`, pop it. Repeatable N times — this is the redo stack we lack today.
- **new commit / pull / manual git op** — self-healing invalidation: any state where `HEAD != top.base` means the timeline moved; clear `future`. No hooks into commit paths needed — the HEAD check *is* the invalidation.
- **undo after redo** — works naturally: `HEAD~1` from the restored position is the state just left; it pushes a fresh `future` entry, so undo↔redo stays symmetric.
- **published-tip / dirty-tree / root guards** — unchanged from today (`--force` still required to undo a pushed tip; redo needs no published guard — it restores a commit that already was the tip).

### Why a file, not more reflog parsing

`HEAD@{1}` only sees one step back and can't express "the next redo after a redo" (each reset overwrites the @{1} window). The journal makes multi-level redo explicit, inspectable (`myco undo --list` can render it), and survives restarts. Reflog stays as the audit trail; the journal is disposable derived state — corrupt/missing file = empty history, never an error.

### Surfaces

- `myco undo [--force]` — unchanged UX; now records history. `--list` (optional, cheap) prints the redoable stack.
- `myco redo` — pops one level per call; repeatable. Wire payload gains `remaining` depth.
- TUI `u`/`U` — unchanged keys; `U` now works repeatedly. Status line can show `redid … (2 more)` when the stack isn't empty.
- `serve`/`batch` — same verbs, same payloads plus depth.

### Failure modes

- Concurrent myco writers — already serialized by `lock_worktree`.
- External `git reset`/`commit` — invalidates via the `base` check; safe.
- Two clones/machines — last writer wins on the file; documented limitation, same class as any local state.
- Journal lost (git dir wiped) — redo just refuses "nothing to redo"; ledger unaffected.

### Migration

No migration. Repos with no journal behave exactly as today for undo; redo falls back to the legacy reflog check once (or refuses — decide at impl; fallback is ~10 lines, keep it for one release).

### Test plan

- ops: undo×3 → redo×3 restores exact hashes; redo×4 refuses; new commit invalidates; dirty tree guards; corrupt journal self-heals; undo-after-redo symmetry.
- CLI e2e + wire handler; TUI `U` repeat.
- Property: random op sequences (add/done/undo/redo/commit) never lose commits — every redo target is an ancestor-or-former-tip of the chain.
