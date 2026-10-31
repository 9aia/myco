+++
id = "I-021"
title = "audit lows: store/file-spec edges"
status = "open"
created = "2026-10-31"
priority = "low"
labels = ["audit"]
+++

body.trim() mangles leading indented code blocks (itemfile.rs:273, doc.rs:107, handoff.rs:167,241 — whitespace-only notes also break round-trip). emit uses trim_end only → hand-built models don't fixed-point. Self-referential blocks:/supersedes unchecked anywhere (line.rs:150). parse_item_filename 9-digit cap vs unbounded emit; handoff filename no cap (itemfile.rs:78,59, handoff.rs:41). expect("non-empty") sites todo.rs:65/backlog.rs:56/legacy.rs:174. Unbounded recursion in emit/parse/tree paths. status:deferred accepted on TODO.md lines (line.rs:191). take drops emptied NAMED sections though they emit fine (ledger.rs). Item.created serializes null in JSON (item.rs:44). frontmatter rejects priority="med" and unquoted TOML dates (itemfile.rs:124).
