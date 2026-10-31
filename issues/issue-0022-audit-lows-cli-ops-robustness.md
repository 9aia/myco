+++
id = "I-022"
title = "audit lows: cli/ops robustness"
status = "done"
created = "2026-10-31"
priority = "low"
labels = ["audit"]
+++

absorb parses rg output linewise — filenames with :/\n forge findings (absorb.rs:347-352; use rg --null/--json). migrate pm/ deletes files before commits land — failure leaves uncommitted deletion (migrate.rs:257-270). init creates orphan branch before worktree add — failure leaves partial state (init.rs:63-75). main_worktree needs git >= 2.31 — older git silently falls back to worktree.parent() (myco-git lib.rs:114-127). $PAGER/$MYCO_PAGER via sh -c — env-controlled exec, document it (ops.rs:148-162).

## Resolution

fixed e86db8d — rg --null+exists, migrate rollback, init branch rollback, main_worktree_compat, pager doc
