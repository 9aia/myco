+++
id = "I-020"
title = "myco next — emit highest-pri open item"
status = "open"
created = "2026-10-31"
priority = "medium"
labels = ["agent-dx"]
+++

Falls out of P-006 (declarative views). No verb answers "what
should I work on" — priority orders the open lists, but nothing
dequeues.

`myco next` emits the single highest-pri open item
(`pri_then_id` ordering, tasks+issues by default), one line +
`--json` payload. Flags reuse the P-006 filter struct
(`--kind`, `--label`, `--epic`, …) so `next` is literally
"view with limit 1". Optional `--claim` claims it in the same
mutation.

Depends on P-006's filter engine; trivial after it.
