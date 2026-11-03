+++
id = "I-029"
title = "project worktree detached — 22 arca ledger commits unreachable"
status = "done"
created = "2026-11-03"
priority = "high"
labels = ["audit"]
+++

Found while auditing the fleet: `arca/project` was on a **detached HEAD** with 22 auto-commits ahead of `origin/project` and no `refs/heads/project` — the ledger history reachable only through that worktree HEAD, invisible to `git log project`, never pushed. Nothing prevented recurrence: discovery accepts any dir holding `TODO.md`, and `commit_all` committed wherever HEAD pointed. Fixed systemically in ADR-0003: `check` reports worktree-detached/branch/unpushed, `--fix` reattaches with `checkout -B project`, and every mutating verb refuses a detached ledger before writing. Fleet outcome: all six 9aia ledgers exit 0 from `myco check`.
