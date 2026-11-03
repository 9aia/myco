+++
id = "P-007"
title = "verb registry + config-declared verbs"
status = "in-progress"
created = "2026-11-02"
owner = "devin"
priority = "medium"
+++

Status: **draft** · Created: 2026-11-02

## Goal & Overview

The verb surface is compile-time and triplicated: clap subcommands (truth), `manifest().verbs` (hand array), `serve::METHODS` (hand array) — already drifted (serve lacks `epic`, `milestone`, `index.*`, `org.*`, `absorb`, `init`, `migrate`, `commit`). Meanwhile views are config (`[views.*]`, layered) — verbs should get the same treatment.

Make the verb surface data: **one builtin registry** generates clap + manifest + serve + batch; **`[verbs.*]` in `myco.toml`** declares repo/user verbs, layered like views.

## Requirements

- Single verb registry — name, help, param schema, kind-lock, handler. `Cli::command()` built from it; manifest/serve/batch read the same table. A verb can't exist on one surface and not the others.
- `[verbs.<name>]` merges across config layers like `[views.*]` — repo verbs shared, user verbs personal; higher precedence wins collisions.
- Two config verb kinds, both read-only by construction:
  - `alias` — name → builtin + baked args
  - `view-emit` — name → ViewSpec + emit mode (`table|json|ids|first`); `first` gives I-020 (`myco next`) for free
- Mutations stay builtin — state machine, id alloc, locking, commit semantics aren't config data.
- serve `initialize` advertises registry ∪ repo's config verbs — method tables become per-repo.
- Collisions mirror view-shadowing: builtin wins, warn.

## Technical Design

- Registry entry: `{ name, group, about, params: &[Param], kind_lock: Option<ItemKind>, handler }`. clap built via `Command::new` (builder, not derive); params schema doubles as the serve/batch JSON schema.
- Dispatch: clap → registry handler; serve/batch `match method` → registry lookup, config verbs checked after builtins. One dispatch path, three front-ends.
- Config verb validation at load: unknown alias target → warn + skip; ViewSpec validated by existing `view` machinery.
- Precedence: builtin > repo config > user config.
- Precedent already in the codebase: `[views.board]` config shadowing warns and still reaches the wire — same pattern, same warning path.

## Open questions

- **Tier 3 — composite verbs** (sequence of verbs with arg-forwarding)? Deliberately out for v1 — needs ordering + failure semantics; alias covers the common case.
- Can config verbs *hide* builtins (org policy)? Probably no — stable surface is part of the audit trail.
- serve config freshness: per-request re-read or pinned at spawn? Views resolve per call today — keep parity.
- Registry home: `myco-ops` (plumbing) vs `myco-cli` (verbs) — likely spec in ops, handlers in cli.
- Builder-mode clap loses derive-time checks on global flags — acceptable?

## Related

- I-020 `myco next` — falls out of `view-emit`/`alias` + `emit = "first"`
- P-004 batch — already one dispatch path (serve surface), extends naturally
- Audit 2026-11-02: monomorphic ops (`&MdStore`) is the other decoupling gap — separate item
