---
url: /myco/explanation/architecture.md
---
# Architecture

Ports-and-adapters over a flat `crates/` workspace:
`myco-core` is pure domain — item models, the id registry, the
state machine — with no I/O. Ports are traits (`myco-store`,
`myco-config`, `myco-index`); impls live in separate crates;
`crates/myco-cli` (package `myco`) is the composition root that
wires impls to verbs.

Design rules:

* **No `unsafe`, no async** — the binary stays dependency-light
  and synchronous; no runtime to embed.
* **Git via the `git` CLI** — `myco-git` shells out for the
  orphan branch, worktree, and per-verb commits; no
  libgit2/gix binding.
* **Markdown is the file spec** — `myco-store-md` owns strict
  parse/emit: deterministic output, stable ordering, minimal
  diffs. `myco-testkit` freezes emit with golden fixtures and
  proptest round-trips, so the format can't drift silently.
* **The index is derived, never canonical** — `myco-index-redb`
  snapshots ledgers into redb for `status`/`view`/`org`; it can
  be deleted and rebuilt at any time.

| crate | layer | purpose |
|---|---|---|
| `myco-core` | domain | myco domain core: item models, ID registry, state machine — pure, no I/O |
| `myco-config` | port | Config port: where myco.toml-shaped config comes from |
| `myco-index` | port | Index port: the derived, disposable cache powering status/board/report/org views |
| `myco-store` | port | Store port: where project/ state (TODO.md, BACKLOG.md, meta.toml, item files) is persisted |
| `myco-ops` | ops | myco — ops I/O shell: project discovery, store open/save, commit-per-mutation, verb plumbing |
| `myco-config-toml` | adapter | Config impl: myco.toml file (explicit path, env, ./, ~/.config) |
| `myco-driver` | adapter | tamias-driver-myco — pavo view driver over a repo's project/ ledger (reads via myco-ops) |
| `myco-git` | adapter | Git plumbing for myco — shells out to the git CLI; owns the project branch/worktree mechanics |
| `myco-index-redb` | adapter | Index impl: redb snapshots + repos.toml registry under ~/.local/share/myco (disposable derived cache) |
| `myco-shell` | adapter | Run a command in a working directory via bash -c — the generic shell-out helper |
| `myco-store-md` | adapter | Markdown adapter for the store port — strict parse/emit of project/ files, round-trip stable; owns the file spec |
| `myco-testkit` | adapter | Golden fixtures + conformance: frozen md emit snapshots and proptest round-trips for the project/ file spec |
| `myco-tui` | adapter | myco — interactive board TUI (ratatui); a thin front-end over myco-ops |
| `myco` | binary | myco — records engine for agent work — views + mutations; project management is the shipped prelude |

## The engine and the prelude

The shape that emerged rather than being designed: **myco is a
records + views + mutations engine, and PM is its shipped prelude**
(P-009). `myco init` seeds that prelude — the five `[[kinds]]`, the
builtin views, the registry verbs — so editing config isn't
customizing a PM tool, it's programming an engine whose default
program happens to be project management.

```text
            engine (code)                      prelude (seeded config)
  ┌────────────────────────────────┐    ┌───────────────────────────────┐
  │ sources   repo │ index         │    │ [[kinds]]  task issue …       │
  │ eval      filter→group→sort    │    │ [views]    board open hot …   │
  │ emits     list board md json   │    │ [verbs]    alias view-emit …  │
  │ dispatch  one registry table   │    └───────────────────────────────┘
  └────────────────────────────────┘
      owns, must never be data            declared, layered, shadowable
```

A read is `universe → eval → emit`:

| stage | knob | values |
| --- | --- | --- |
| universe | `source` | `repo` (this worktree) · `index` (the registered-repo union) |
| eval | `filter → group_by → sort → limit` | a `ViewSpec`, from config or flags |
| emit | `format` | `list` · `board` · `md` · `json` · `repos` |

Every feature gets asked one question — **engine or prelude?**
Engine answers generalize (a new source, axis or format); prelude
answers configure (which kinds, views and verbs ship seeded).

### What stays code

The moat is what must be *owned* rather than declared: the markdown
file spec, atomic commit-per-mutation on `project`, the state
machine, the id registry. `Item` keeps fixed semantics — a schema
DSL over arbitrary fields would make this jq-over-markdown, a
different and worse product. Mutations stay code as well: the
prelude configures *what* exists, never *how* writes behave
(ADR-0004, one registry).

## Boundary — work memory only

myco owns the ledger of intent (tasks, issues, proposals, plans,
ADRs, handoffs). Neighbors own their own memory:

* **sturn** — ephemeral coordination (live claims/ops/topics)
* **kuda** — code memory; myco indexes work items, not code
* **peregrin** — orchestration; it embeds the ledger through
  `myco serve` (JSON-RPC 2.0 on stdio — `adrs/0001`), reachable
  one-shot via `myco batch`
* **bupha** — CI gate; `myco check` in pipelines, `Item:` trailers
* **mycena** — surfaces TODO/FIXME in code → `myco absorb`
* **sepia** — session lifecycle → `myco handoff` / `myco resume`

Anything needing a daemon, a multi-machine view, or arbitration
between actors belongs to peregrin, not here.
