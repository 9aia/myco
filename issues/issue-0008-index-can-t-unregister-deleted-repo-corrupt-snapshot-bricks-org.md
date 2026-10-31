+++
id = "I-008"
title = "index: can't unregister deleted repo; corrupt snapshot bricks org"
status = "done"
created = "2026-10-31"
priority = "high"
labels = ["audit"]
+++

index remove canonicalizes the missing path → Err → entry stuck forever (indexcmd.rs:197-204). One bad JSON snapshot → Error::Codec kills org/index list (index-redb:215-217). Fix: tolerate missing paths; stale→rebuild not fatal.

## Resolution

fixed 899431e — registered_key fallbacks for deleted repos; corrupt snapshot→rebuild
