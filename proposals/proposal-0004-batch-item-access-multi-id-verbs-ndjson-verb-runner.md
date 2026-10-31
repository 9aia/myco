+++
id = "P-004"
title = "batch item access — multi-id verbs + NDJSON verb-runner"
status = "open"
created = "2026-10-31"
priority = "medium"
labels = ["agent-dx"]
+++

## Context

`status --prompt` sizes up the ledger in one call — but the next
step is a read fan-out: `resume`/`item show` once per item, each
re-discovering the worktree and framing its own output. Briefing on
6 items costs 6 spawns and ~6x the tokens. `myco serve` (ADR-0001)
is a request/response surface, but only for a process that can hold
a pipe open and frame JSON-RPC — unusable one-shot from a shell.

## Plan

1. **Multi-id reads** (S): `myco resume <ID>…` / `item show <ID>…`
   take 1..n ids — one open, per-item blocks in order; `--json` =
   `[{id, ok, payload|error}]`, bad ids inline (exit nonzero if any
   failed). Single-id shape unchanged.
2. **`myco batch`** (M): one-shot `myco serve` — NDJSON requests on
   stdin/`--file`, NDJSON responses out, same `WireProtocol` +
   `dispatch`. Full served verb surface (incl. mutations, per-request
   commits) without holding a pipe open; per-line errors, never
   fatal.

## Non-goals

- Batch-array framing inside `serve`; multi-verb transactions —
  ADR-0001 follow-ups.
- Multi-id mutation sugar (`todo done T-1 T-2`) — part 2 covers it.
- Org/index methods — out per ADR-0001.
