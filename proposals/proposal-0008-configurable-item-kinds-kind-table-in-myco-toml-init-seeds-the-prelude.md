+++
id = "P-008"
title = "configurable item kinds — kind table in myco.toml, init seeds the prelude"
status = "done"
created = "2026-11-02"
owner = "devin"
priority = "medium"
labels = ["agent-dx"]
+++

## Context

Item kinds are a closed set: task, issue, proposal, adr, plan —
hardcoded in `ItemKind`, each carrying a fixed ID prefix
(`T-`/`I-`/`P-`/`ADR-`/`PL-`), storage shape (doc section vs
`files/<kind>/` vs `archive/`), state machine, and verb set
(`done`/`resolve`/`supersede`). That grammar is currently what
makes `check`, `org`, `absorb`, and the index meaningful — but it
also means every user's PM vocabulary must fit our five.

The read surface just proved the pattern: fixed presets, config
shadows. Apply the same move to kinds — a schema table in
`myco.toml`, with `myco init` seeding today's five as the
**prelude** so zero-config behavior is unchanged.

## Sketch

```toml
[[kinds]]
name      = "task"
prefix    = "T"
storage   = "doc"        # TODO.md/BACKLOG.md sections
states    = ["open","claimed","in-progress","blocked","deferred","done"]
done_state = "done"

[[kinds]]
name      = "issue"
prefix    = "I"
storage   = "dir"        # issues/issue-0003-slug.md
states    = ["open","claimed","in-progress","blocked","done"]
done_state = "done"
resolve_verb = true      # `issue resolve` vs `todo done`

[[kinds]]
name      = "adr"
prefix    = "ADR"
storage   = "dir"
supersede = true         # superseded/supersedes frontmatter
zero_pad  = 4
```

- `ItemKind` becomes data — the registry reads the table; id
  grammar (`<prefix>-<zero_pad:N>`) is declared per kind.
- The global state machine becomes per-kind `states` +
  `done_state`; transitions validate against the kind's list.
- `storage` picks the file backend (`doc` sections | `dir`
  files); verbs gate on kind capabilities (`supersede`,
  `handoff`).
- User kinds get everything for free: views (`kind` filter),
  `item set`, handoffs, index snapshots — the infra is generic
  already.

## Non-goals (v1 of this)

- No new top-level noun verbs per user kind (`myco spike new`)
  — `item` verbs cover them; prelude kinds keep theirs.
- Kind migration/renames — out; declare, don't migrate.
- Cross-kind identity moves — out.

## Order

1. Kind table parse + validation (`myco-core`), prelude table
   as compiled-in default so missing config = today.
2. Registry/state machine reads the table.
3. Storage backends honor `storage`.
4. `init` writes the prelude into `myco.toml` (commented).
5. Rip: `check`, `absorb`, `serve` METHODS, index, org.

## Risks

This is P-001-scale — the file spec, `check`, `absorb`, serve's
per-kind methods (`issue.list`, `adr.list`), and org snapshots
all key off the closed enum today. Each needs a kind-table
read path before the enum can go.
