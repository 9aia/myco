+++
id = "I-006"
title = "meta.toml typos silently reset id counters — id reuse"
status = "open"
created = "2026-10-31"
priority = "high"
labels = ["audit"]
+++

NextCounters/ProjectMeta lack deny_unknown_fields + serde(default) next (core/meta.rs:21-37, store-md/meta.rs:13). One-char typo resets counters → done ids reissued → handoff/history collisions. Also t=0 mints T-000 (unemit-able id). Fix: deny_unknown_fields + counter>=1.
