+++
id = "I-005"
title = "TUI: mutation errors fatal-exit; stale-ledger clobber"
status = "done"
created = "2026-10-31"
priority = "high"
labels = ["audit"]
+++

ITEMS-board keys route to ledger.locate → Err → app exits (app.rs:293,421+); all mutators but reorder propagate errors as exit; writes save whole startup snapshot, clobbering concurrent CLI changes. Fix: errors→status bar, board-scoped mutation table, per-mutation reload under lock.

## Resolution

fixed 6574832 — mutate() routes errors to status bar; FILES read-only; per-mutation re-read
