+++
id = "I-015"
title = "check: missing vs unreadable conflated; parse-fail skews findings"
status = "done"
created = "2026-10-31"
priority = "medium"
labels = ["audit"]
+++

read() maps all errors to None (check.rs:103-105) → unreadable meta.toml reported missing-file and --fix scaffolds OVER it. Non-UTF8 .md/handoff files skipped silently (check.rs:771,967). Unparsable TODO.md → ids unregistered → false dangling-* + renumber mints dups (check.rs:598-702,1079). Fix: distinguish errors, findings for unreadable, exclude parse-failed files from semantic pass.

## Resolution

fixed 87288a1 — unreadable-file finding, no scaffold-over, parse-fail isolation
