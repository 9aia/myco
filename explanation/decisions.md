# Decisions

ADR-lite: the calls that shaped myco, the alternatives rejected, and
the consequences we live with. Deeper versions live in the ledger's
own `adrs/` — this page is the map, not the archive.

## Markdown is canonical; the index is disposable

**Decision:** `project/` files are the source of truth; the redb
index is a derived cache that `status`/`view`/`org` read through and
`index rebuild` regenerates.

**Alternatives:** a database as the store; git as the only read path
(no index at all).

**Consequence:** the ledger is human-editable, diffable, and survives
index corruption — at the cost that every read pays a snapshot sync
and `check` must reconcile drift between the two.

## An orphan branch + worktree, not files on `main`

**Decision:** the ledger lives on a `project` orphan branch mounted
as a `project/` worktree; code branches gitignore it.

**Alternatives:** ledger files committed on `main`; a separate repo
per ledger; a `.myco/` dir inside the code tree.

**Consequence:** ledger commits never pollute code history and PRs,
and both trees stay checked out at once — but it requires git
worktree machinery, and a detached HEAD silently breaks the audit
trail (enforced by `check` + mutating-verb refusal, ADR-0003).

## One registry of verbs; the prelude is config

**Decision:** mutations are code in one verb registry; what exists —
kinds, views, verb aliases — is layered config (`myco.toml`).

**Alternatives:** per-kind verbs hard-coded; a schema DSL over
arbitrary item fields; user-defined mutation logic.

**Consequence:** `myco init`'s seeded prelude makes PM the default
program of a general records engine (P-009), and custom kinds ride
the generic surface for free — but mutations can never be redefined
by config, which is the point (ADR-0004).

## Commit-per-mutation on `project`

**Decision:** every mutating verb lands as one atomic conventional
commit; `--no-commit`/`MYCO_NO_COMMIT` batches for scripted flows.

**Alternatives:** write files, commit on demand; a WAL outside git.

**Consequence:** `git -C project log` *is* the audit trail — no
separate store to keep honest — at the price of one commit per verb
(noise is accepted; the branch is orphaned precisely so it doesn't
matter).

## No cargo binary; the command is a tamias shim

**Decision:** `myco-cli` is lib-only; the `myco` command ships as a
tamias pack whose `bins[]` shim dispatches `cli` → the engine.

**Alternatives:** a `[[bin]]` in `myco-cli`; crates.io releases.

**Consequence:** install/update lifecycle is tamias's problem
(`tamias install 9aia:myco@<ver>`), and the engine is drivable
in-process by `myco-driver` — but there is no `cargo install myco`,
and org policy keeps the crates off crates.io.

## Git through `lasius-vcs`, not a git library

**Decision:** all git access goes through the shared `lasius-vcs`
port (git CLI impl) inside `myco-ops::vcs`.

**Alternatives:** git2/gix bindings; shelling out ad hoc per call.

**Consequence:** one git implementation org-wide, no binding
dependency or version drift — at the cost of depending on a `git`
binary on PATH and parsing CLI behavior.
