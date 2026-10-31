+++
id = "I-002"
title = "has_changes swallows git errors — silent uncommitted writes"
status = "open"
created = "2026-10-31"
priority = "high"
labels = ["audit"]
+++

git errors map to false (myco-git/lib.rs:131) → autocommit returns Ok(false) → verbs report success, ledger and history diverge. Same swallow makes missing git binary read as not-a-repo (lib.rs:14-23). Fix: Result<bool>.
