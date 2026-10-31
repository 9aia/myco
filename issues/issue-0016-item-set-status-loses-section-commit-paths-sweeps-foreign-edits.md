+++
id = "I-016"
title = "item set --status loses section; commit_paths sweeps foreign edits"
status = "open"
created = "2026-10-31"
priority = "medium"
labels = ["audit"]
+++

mutate_item does take + put_todo(item, None) → drops item into sections[0], not its origin section (itemcmd.rs:286-291). commit_paths scopes by file not hunk → pre-existing edits inside AGENTS.md/.gitignore get committed under myco's message (ops.rs:102-120). Fix: preserve section loc; only add paths myco wrote.
