# Concepts

The vocabulary of the myco ledger — each term defined once.

## The ledger

- **Ledger** — the `project/` worktree: the canonical record of intent
  for one repo. Markdown files on an orphan `project` branch; git
  history is the audit trail.
- **Item** — one unit of work or thought, with a prefixed id
  (`T-001` task, `I-001` issue, `P-001` proposal, `PL-001` plan,
  `ADR-0001` decision). Tasks are lines in `TODO.md`/`BACKLOG.md`/
  `DONE.md`; the file-backed kinds get one markdown file each.
- **Kind** — the item type (`task`, `issue`, `proposal`, `adr`, `plan`,
  plus `[[kinds]]` customs). Each kind has a prefix, a storage shape
  (`doc` lines or `dir` files), and its own state vocabulary.
- **State machine** — the seven statuses (`open`, `claimed`,
  `in-progress`, `blocked`, `deferred`, `done`, `cancelled`) and the
  legal transitions between them. Verbs walk it one step at a time.
- **Handoff** — an append-only dated note on an item
  (`handoffs/<ID>.md`) recording done/next/open at session end.
  `resume` reads the latest entries back as restart context.
- **View** — a declarative read spec (`filter → group_by → sort →
  limit → format`) named in `[views]` config or built in (`board`,
  `open`, `epic`, `burnup`, …). Views are the *only* read surface —
  there are no per-view verbs.
- **Verb** — a mutation or read the engine owns (`todo add`,
  `item set`, `handoff`, `view`, …). Every mutating verb is one
  atomic commit on `project`.

## Engine and prelude

- **Engine** — the code-owned half: file spec, state machine, id
  registry, commit-per-mutation, view evaluation. Must never be data.
- **Prelude** — the seeded config half: the five builtin `[[kinds]]`,
  the builtin views, the `[verbs]` aliases `myco init` writes into
  `myco.toml`. PM is the shipped prelude, not the product boundary.
- **Index** — the derived, disposable cache (redb + `repos.toml`
  under `~/.local/share/myco`) powering `status`/`view`/`org` reads.
  Markdown stays canonical; the index can be deleted and rebuilt.
- **Universe** — the set of repos an index-sourced view spans: lasius
  workspace clones ∪ `repos.toml` registrations.
- **Ledger state** — per universe repo: `local` (own `project/`),
  `none` (ledger-less — verbs resolve to the org ledger), or `org`
  (the one repo every `none` member points at).

## Surfaces

- **`myco` command** — a tamias `bins[]` shim: generated script →
  `tamias bin exec` → `tamias-driver-myco cli` → the engine lib.
  There is no cargo-installed `myco` binary.
- **`tamias-driver-myco`** — the driver binary: pavo view driver +
  engine ops surface (`ops` manifest, `cli.<verb>` queries, `cli`
  shim passthrough). Writes always ride the engine's verbs.
- **`myco ui`** — the ratatui board TUI, a front-end over `myco-ops`.
- **`myco serve` / `myco batch`** — the embedding surfaces: ndjson
  JSON-RPC over stdio for daemons, and one-shot batched requests for
  scripts and agent fan-out reads.
