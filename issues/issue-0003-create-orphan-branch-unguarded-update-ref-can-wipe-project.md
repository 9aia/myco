+++
id = "I-003"
title = "create_orphan_branch unguarded update-ref can wipe project"
status = "done"
created = "2026-10-31"
priority = "high"
labels = ["audit"]
+++

update-ref refs/heads/<name> <commit> with no old-value guard (myco-git/lib.rs:96-99). TOCTOU past branch_exists; any call resets project to empty root, orphaning the ledger. Fix: update-ref <ref> <new> empty-string-old = create-only.

## Resolution

fixed 1cd2d80 — create-only update-ref with old-value guard
