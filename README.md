# myco

*Project management, grown not built.*

A mycorrhizal network for your agents: tasks, signals, handoffs.
Agent-first PM — a single `myco` CLI owns every deterministic write
to a repo's `project/` worktree; the skill teaches agents the verbs,
not file formats. Markdown is canonical and human-editable; history
on the `project` branch is the audit trail.

```
agent ──► myco <verb> ──► write md ──► commit on `project` branch
              │                        (one atomic commit per verb)
              └──► --json everywhere + manifest/schema self-discovery
```

## Features

- **work memory, not a tracker product** — the ledger of intent:
  todos, backlog, issues, proposals, plans, ADRs, handoffs
- **hybrid store** — markdown in `project/` is canonical; a redb
  index is a disposable derived cache powering board/report/org views
- **agent-first wire** — `--json` on every verb plus `myco manifest`
  and `myco schema` for self-discovery; `myco status --prompt` is a
  token-sized digest
- **continuity** — `myco handoff <id>` attaches resumable state at
  session end; `myco resume <id>` returns the minimal context to
  restart work
- **numbered slugs, one registry** — `T-012`, `I-003`, `P-002`,
  `ADR-0004`, `PL-001`; state machine owned by `myco-core`, invalid
  states unrepresentable
- **auto-commit per verb** — agents never touch git; every mutation
  is one atomic commit on the `project` branch (`--no-commit` opt-out)
- **ports-and-adapters crates** — `myco-core` (pure domain) +
  `myco-config`/`myco-git`/`myco-shell` infra today; store-md,
  index-redb, template, testkit land per phase

## Position in the stack

myco = **work memory**. Neighbors: **sturn** (ephemeral
coordination), **kuda** (code memory), **peregrin** (orchestration),
**bupha** (CI gate), **mycena** (TODO/FIXME surfacing),
**sepia** (session lifecycle → handoff/resume).

## Status

Phase 1 bootstrap — the bupha anatomy, pruned to PM infra. The verb
surface is stubbed; `myco manifest` and `gen-cli-docs` work today.
Roadmap: `project/proposals/v2.md` (phases 2–9).

## Install

```sh
cargo install --git https://github.com/9aia/myco myco
cargo xtask install        # from a checkout → ~/.cargo/bin
```

## Docs

Docs site lands on the `docs` branch (VitePress + Diátaxis →
GitHub Pages). Repo docs:
[AGENTS](AGENTS.md) · [CONTRIBUTING](CONTRIBUTING.md) ·
[SECURITY](SECURITY.md) · [LICENSE](LICENSE) (MIT) ·
[project](project/index.md) — TODO, backlog, ADRs, proposals
