+++
id = "I-007"
title = "check --fix rename clobbers; renumber misses archive/"
status = "done"
created = "2026-10-31"
priority = "high"
labels = ["audit"]
+++

fs::rename replaces existing targets silently (check.rs:1257-1266) → destroys canonical files on slug collisions. Renumber repoints handoffs/<old>.md but not handoffs/archive/<old>.md (check.rs:1190-1201). Fix: exist-check/uniquify; cover archive.

## Resolution

fixed 87288a1 — rename refuses existing targets; archive/ covered by renumber
