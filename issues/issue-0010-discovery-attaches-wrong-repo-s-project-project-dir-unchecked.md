+++
id = "I-010"
title = "discovery attaches wrong repo's project/; project_dir unchecked"
status = "open"
created = "2026-10-31"
priority = "medium"
labels = ["audit"]
+++

Ancestor walk runs before enclosing-worktree check (discover.rs:99-110) — inside a worktree, another repo's project/ wins; absorb then rg-scans wrong root. worktree_branch matches worktree SUBDIRS too. project_dir accepts absolute/../ paths (discover.rs:87-95). Fix: cap walk at toplevel, root-only branch check, relative-only project_dir.
