+++
id = "I-012"
title = "store integrity: id cross-check, all_ids, emit validation, panics"
status = "done"
created = "2026-10-31"
priority = "medium"
labels = ["audit"]
+++

filename NNNN vs frontmatter id never checked (itemfile.rs:205-214) → alloc_id reissues live ids. all_ids swallows parse+IO errors (store.rs:130-134). emit_item_file no validation → emit→parse breaks (itemfile.rs:283-309). Ledger::item/take panic on stale TaskLoc (ledger.rs:74-129). walk_to/NewItemMeta::apply mutate-then-fail (walk.rs:73-81, item.rs:261-278). Fix: cross-check, Result, emit validation, Option, validate-then-apply.

## Resolution

landed — all_ids Result, filename↔frontmatter check, emit validation, Option TaskLoc, atomic applies (verified in tree)
