+++
id = "P-004"
title = "batch item access — multi-id verbs + NDJSON verb-runner"
status = "open"
created = "2026-10-31"
priority = "medium"
labels = ["agent-dx"]
+++

## Context

Token thrift is the product's pitch — `status --prompt` exists so an
agent can size up the ledger in one call. But the next step in every
session is a read fan-out: `resume`/`item show` once per candidate
item, and each call re-discovers the worktree, re-opens the store,
and frames its own output. Briefing on 6 items costs 6 process
spawns, 6 envelopes, ~6x the tokens of one combined answer.

`myco serve` (ADR-0001) already *is* a sequential request/response
verb surface — but only for a process that can hold a pipe open and
frame JSON-RPC. A shell agent can't reasonably do that one-shot, and
ADR-0001 explicitly deferred JSON-RPC batch-array framing.

## Plan — two parts

1. **Multi-id read verbs** (S): `myco resume <ID>…` and
   `myco item show <ID>…` accept 1..n ids — one discover+open, one
   output. Human output is the existing per-item block repeated in
   order; `--json` becomes `[{id, ok, payload|error}]` — a bad or
   missing id reports inline (exit nonzero if any failed) instead of
   killing the whole batch. Single-id output shape stays identical.

2. **`myco batch`** (M): a one-shot `myco serve` — read NDJSON verb
   requests from stdin or `--file`, dispatch each through the same
   ops the serve method handlers use, write one NDJSON response line
   per request. `printf '{"id":1,"method":"resume","params":{"id":"T-1"}}\n' | myco batch`
   gives an agent the whole served verb surface (incl. mutations)
   without holding a pipe open. Per-line errors, never fatal;
   mutations keep serve's per-request commit semantics. Reuses
   ADR-0001's `WireProtocol` + `dispatch` — no new method surface.

## Non-goals

- JSON-RPC batch-array framing inside `myco serve` — an ADR-0001
  follow-up, orthogonal to this.
- Multi-verb transactions / all-or-nothing atomicity — stays
  deferred with ADR-0001's "multi-request transactions".
- Multi-id *mutation* sugar (`todo done T-1 T-2`, `item set --status
  done <ID>…`) — nice, but part 2 covers it; only add if it falls
  out of the multi-id read refactor for free.
- Org/index methods — out per ADR-0001, same here.

## Order

Part 1 first (the daily win), part 2 behind it. Issues to be filed
on acceptance.
