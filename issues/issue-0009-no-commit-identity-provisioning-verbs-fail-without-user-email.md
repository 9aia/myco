+++
id = "I-009"
title = "no commit-identity provisioning — verbs fail without user.email"
status = "done"
created = "2026-10-31"
priority = "medium"
labels = ["audit"]
+++

commit-tree/git commit need committer identity; no -c fallback, no preflight (myco-git/lib.rs:93-101,200-202; ops.rs:102-120). Every test sets config so it's masked. Fix: -c user.name=myco fallback or targeted error.

## Resolution

fixed 1cd2d80 — git_ident probes GIT_COMMITTER_IDENT, falls back to myco@localhost
