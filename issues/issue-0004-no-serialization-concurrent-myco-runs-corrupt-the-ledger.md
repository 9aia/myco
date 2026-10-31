+++
id = "I-004"
title = "no serialization — concurrent myco runs corrupt the ledger"
status = "done"
created = "2026-10-31"
priority = "high"
labels = ["audit"]
+++

alloc_id RMW on meta.toml (store.rs:118), ledger load-mutate-save (items.rs:30), repos.toml RMW + shared tmp name (index-redb:126-141) — parallel runs → dup ids, clobbers, interleaved registry. Fix: File::try_lock on project/.lock held load→save→commit; fold registry into redb or same lock.

## Resolution

fixed 899431e — worktree lock (common git dir), exclusive writers/shared readers
