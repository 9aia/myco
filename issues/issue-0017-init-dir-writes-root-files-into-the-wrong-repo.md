+++
id = "I-017"
title = "init --dir writes root files into the wrong repo"
status = "open"
created = "2026-10-31"
priority = "medium"
labels = ["audit"]
+++

repo_root_for derives root from --dir's parent (discover.rs:142-146) — for a worktree outside <root>/project, .gitignore/AGENTS.md land in the parent's repo (init.rs:86-109). Also --dir uncanonicalized (discover.rs:158-160). Fix: resolve owner repo via main_worktree or require --dir under root.
