# Architecture

Ports-and-adapters over a flat `crates/` workspace:
`myco-core` is pure domain — item models, the id registry, the
state machine — with no I/O. Ports are traits (`myco-store`,
`myco-config`, `myco-index`); impls live in separate crates;
`crates/myco-cli` (package `myco`) is the composition root that
wires impls to verbs.

Design rules:

- **No `unsafe`, no async** — the binary stays dependency-light
  and synchronous; no runtime to embed.
- **Git via the `git` CLI** — `myco-git` shells out for the
  orphan branch, worktree, and per-verb commits; no
  libgit2/gix binding.
- **Markdown is the file spec** — `myco-store-md` owns strict
  parse/emit: deterministic output, stable ordering, minimal
  diffs. `myco-testkit` freezes emit with golden fixtures and
  proptest round-trips, so the format can't drift silently.
- **The index is derived, never canonical** — `myco-index-redb`
  snapshots ledgers into redb for `status`/`board`/`org`; it can
  be deleted and rebuilt at any time.

<!-- crate-map -->
| crate | layer | purpose |
|---|---|---|
| `myco-core` | domain | myco domain core: item models, ID registry, state machine — pure, no I/O |
| `myco-config` | port | Config port: where myco.toml-shaped config comes from |
| `myco-index` | port | Index port: the derived, disposable cache powering status/board/report/org views |
| `myco-store` | port | Store port: where project/ state (TODO.md, BACKLOG.md, meta.toml, item files) is persisted |
| `myco-config-toml` | adapter | Config impl: myco.toml file (explicit path, env, ./, ~/.config) |
| `myco-git` | adapter | Git plumbing for myco — shells out to the git CLI; owns the project branch/worktree mechanics |
| `myco-index-redb` | adapter | Index impl: redb snapshots + repos.toml registry under ~/.local/share/myco (disposable derived cache) |
| `myco-shell` | adapter | Run a command in a working directory via bash -c — the generic shell-out helper |
| `myco-store-md` | adapter | Markdown adapter for the store port — strict parse/emit of project/ files, round-trip stable; owns the file spec |
| `myco-testkit` | adapter | Golden fixtures + conformance: frozen md emit snapshots and proptest round-trips for the project/ file spec |
| `myco` | binary | myco — project management, grown not built: agent-first PM CLI |


## Boundary — work memory only

myco owns the ledger of intent (tasks, issues, proposals, plans,
ADRs, handoffs). Neighbors own their own memory:

- **sturn** — ephemeral coordination (live claims/ops/topics)
- **kuda** — code memory; myco indexes work items, not code
- **peregrin** — orchestration; an optional `myco serve` ndjson
  wire is deferred per `adrs/0001`
- **bupha** — CI gate; `myco check` in pipelines, `Item:` trailers
- **mycena** — surfaces TODO/FIXME in code → `myco absorb`
- **sepia** — session lifecycle → `myco handoff` / `myco resume`

Anything needing a daemon, a multi-machine view, or arbitration
between actors belongs to peregrin, not here.
