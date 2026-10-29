+++
id = "ADR-0001"
title = "ndjson serve wire for peregrin embedding"
status = "open"
created = "2026-10-29"
priority = "medium"
+++

# ndjson serve wire for peregrin embedding

Status: **proposed**

## Context

peregrin orchestrates campaigns across 9aia repos; its nodes need
programmatic access to the myco ledger. The CLI already answers
that need — every verb is idempotent, `--json`-shaped, and commits
atomically — but each call spawns a fresh process and re-opens
the store, which is awkward inside a long-lived orchestrator.

There is a direct precedent: sepia exposes `sepia serve`, an
ndjson JSON-RPC wire on stdio so peregrin embeds session
lifecycle without shell-per-call. `myco manifest` already
advertises `serve` in the verb surface; the proposal's open
question was whether the CLI alone is enough.

## Decision

Defer `myco serve` to post-1.0. The CLI is the wire for now:
idempotent verbs, `--json` everywhere, atomic commits — a
campaign node can shell `myco <verb> --json` today with no extra
surface to maintain.

When a persistent channel becomes worth it, `myco serve` will
mirror the verb surface as methods over a sepia-style
SessionPlane ndjson JSON-RPC wire on stdio, with the protocol
versioned through `manifest` (`capabilities.protocol` bumps, the
way `implemented` already tracks the shipped surface).

## Consequences

- No wire protocol to version or keep compatible during the v2
  rewrite — the ledger file spec stays the only contract.
- peregrin integration is unblocked now via ordinary subprocess
  calls; nothing in the orchestrator blocks on a daemon.
- The upgrade path is recorded here: `serve` is a transport shim
  over the same verb semantics, not a second API, so adopting it
  later is additive, not a migration.
