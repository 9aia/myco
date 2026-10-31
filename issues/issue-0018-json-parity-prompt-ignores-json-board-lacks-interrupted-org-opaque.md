+++
id = "I-018"
title = "json parity: prompt ignores --json, board lacks interrupted, org opaque"
status = "done"
created = "2026-10-31"
priority = "medium"
labels = ["audit"]
+++

status --prompt --json ignores --json (status.rs:105-108). board --json lacks interrupted flag (board.rs:418 vs status.rs:110). org status --json error entries lack path/name (org.rs:146-147). issue list --archived --json has no archived marker (items.rs:219-229). item set --blocks lacks value_delimiter (itemcmd.rs:49). Fix: per finding.

## Resolution

fixed 9028c2d — prompt --json, board interrupted flag, org identity, archived marker, comma delimiters
