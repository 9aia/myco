+++
id = "I-011"
title = "markdown escaping: ATX closing ## x ### breaks round-trip"
status = "done"
created = "2026-10-31"
priority = "medium"
labels = ["audit"]
+++

mdx::inline verifies escaping in Paragraph context but emits into ATX headings (mdx.rs:39-62; todo.rs:148). Names ending ' #' emit closing sequences → parse(emit)!=emit. is_valid_session allows space/#/\r → silent mutation + \r→\n splitting (handoff.rs:36, mdx.rs:97). Fix: context-aware validation + tighten session charset; add #/ws to testkit strategies.

## Resolution

fixed 10bacb7 — mdx::heading context-aware escaping, session charset tightened
