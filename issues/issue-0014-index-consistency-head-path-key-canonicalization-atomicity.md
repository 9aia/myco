+++
id = "I-014"
title = "index consistency: head path, key canonicalization, atomicity"
status = "done"
created = "2026-10-31"
priority = "medium"
labels = ["audit"]
+++

current_head hardcodes <root>/project (indexcmd.rs:91-93) → --dir/project_dir repos forever stale. Snapshot keyed on uncanonicalized proj.root vs canonical registered_root (indexcmd.rs:76-79,139-144). remove saves registry before clearing snapshot → orphans on failure (index-redb:182-191); add non-atomic. head_commit unwrap_or_default → '' key. Fix: worktree-aware head, canonicalize once, ordering.

## Resolution

fixed 899431e — registry lockfile+atomic tmp, canonical keys, worktree-aware head, ordering
