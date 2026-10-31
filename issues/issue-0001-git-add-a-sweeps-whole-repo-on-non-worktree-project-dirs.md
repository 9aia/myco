+++
id = "I-001"
title = "git add -A sweeps whole repo on non-worktree project/ dirs"
status = "open"
created = "2026-10-31"
priority = "high"
labels = ["audit"]
+++

commit_all/has_changes run git -C <dir> add -A + status over the ENTIRE repo (myco-git/lib.rs:131,200). Non-worktree project/ dirs (TODO.md fallback, unchecked --dir) → every verb sweeps user's dirty files into chore: commits. Fix: pathspec-scope, qualify --dir.
