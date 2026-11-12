# Myco

Project management, grown not built — agent-first PM in Rust. A
`myco` CLI owns every deterministic write to a repo's `project/`
worktree; the skill (`.agents/skills/myco`) teaches agents the verbs,
not file formats. Markdown is canonical; the index is a disposable
derived cache; the `project` branch history is the audit trail.

<!-- myco:agent-awareness -->

## Agent awareness

- Session start / any work discussion: run `myco status --prompt`
  before proposing next steps — know the board first.
- User describes new work → offer to capture it (`myco todo add`,
  `issue new`, `proposal new`). Don't wait to be asked.
- Picking up a task: `myco resume <ID>` is the briefing.
- Wrapping up: `myco handoff <ID>` anything resumable; keep statuses
  honest (claimed → in-progress → blocked/done).

<!-- /myco:agent-awareness -->

<!-- sturn:agent-awareness -->

## Sturn awareness

- **No PRs.** The pushed branch/commit is the deliverable; review
  streams continuously (sturn WAL), merge is a human/sturn call.
- sturn-wired repo (`sturn.toml` present): `sturn claim "<glob>"
  --ws <ws>` *before* edits — claims surface conflicts in ms.
- Ops via `sturn write|rename|delete`; `sturn snap` folds the ws
  into a real commit; `sturn push` publishes heads + refs/sturn/*.
- `sturn leave` on exit — drops the actor's claims.

<!-- /sturn:agent-awareness -->

<!-- kuda:agent-awareness -->

## Kuda awareness

- `.kuda/` cards are committed artifacts — read them before
  re-exploring (`kuda map`, `kuda card <mod>`); `kuda status`
  reports per-card drift vs HEAD.
- Code changes that invalidate a card → regenerate it (`kuda
  sync`) in the same change; cards ride the diff they describe.
- `.kuda/index.sqlite` is gitignored + rebuildable; the `*.yaml`
  cards are the checked-in source of truth.

<!-- /kuda:agent-awareness -->


## Layout

Ports-and-adapters, flat `crates/` workspace. `myco-core` is pure
domain (no I/O); ports are traits, impls are separate crates;
`crates/myco-cli` (package `myco`) is the composition root.

| Crate | Role |
| --- | --- |
| `crates/myco-core` | Item models, ID registry, state machine, deps, the `Config` model — pure, no I/O |
| `crates/myco-shell` | `bash -c <cmd>` in a workdir — generic shell-out |
| `crates/myco-store` | Store port — the `project/` persistence trait |
| `crates/myco-store-md` | Markdown impl — strict parse/emit; owns the file spec |
| `crates/myco-index` | Index port — the derived, disposable cache (registry + snapshots); `LedgerMode` re-exported from `myco-core` |
| `crates/myco-index-redb` | Index impl — redb snapshots + `repos.toml` under `~/.local/share/myco` |
| `crates/myco-ops` | Ops I/O shell — discovery, store open/save, commit-per-mutation, verb plumbing; pure pieces live in core. Owns the `myco.toml` loader (`config::TOML` — scarab-config-file discovery + scarab-config-toml parsing over myco's two-layer merge; the `myco-config*` crates are gone) and the index `universe` (lasius clones ∪ `repos.toml`, P-026; deps: `scarab-config`, `scarab-config-file`, `scarab-config-toml`, `lasius-core`) and `vcs` (all git access through the shared `lasius-vcs` port — `lasius-vcs-git` impl; ledger policy like the `myco` committer fallback and `commit_all` lives here, deps: `lasius-vcs`, `lasius-vcs-git`) |
| `crates/myco-testkit` | Golden fixtures + conformance for the file spec |
| `crates/myco-driver` | `tamias-driver-myco` — pavo view driver over `project/` + engine ops (`ops` manifest derived from the `VERBS` table, `cli.<verb>` wire queries re-exec `run`) **plus the `cli` bins[] shim passthrough** (strips `--dir`/`--name` → `myco::run_from`, inherited stdio — the `myco` command itself) and an optional `mcp` surface feature; writes always ride the engine's own verbs |
| `crates/myco-tui` | `myco ui` — interactive board TUI (ratatui) over `myco-ops`; default `tui` feature on `myco-cli` |
| `crates/myco-cli` | The `myco` engine (package `myco`, **lib only — no `[[bin]]`**) — `src/lib.rs` is the engine (`run() = dispatch(&parse())`, `parse_from`/`run_from` for drivers); clap, `--json` everywhere, `manifest`/`gen-cli-docs`. The `myco` command ships as the tamias pack in `libraries/myco/` (T-006); engine e2e tests live in `myco-driver/tests/` and spawn `tamias-driver-myco run` |
| `libraries/myco` | the tamias pack — `tamias.yaml` declares `bins: [{name: myco, shim: cli, driver: myco}]`; `cargo xtask pack` stages `drivers/bin/<triple>/tamias-driver-myco` (gitignored) |
| `xtask` | check / test / coverage / install / pack / site |

Planned (per `project/proposals/proposal-0001-*.md`): `myco-template`.

## Boundary

myco owns **work memory** — the ledger of intent (tasks, issues,
proposals, plans, ADRs, handoffs). Neighbors own theirs:

- **sturn** — ephemeral coordination (live claims/ops/topics).
- **kuda** — code memory; myco indexes *work items*, not code.
- **peregrin** — orchestration; it embeds the ledger through
  `myco serve` (JSON-RPC 2.0 on stdio — ADR-0001).
- **bupha** — CI gate (was: `myco check` in pipelines,
  `Item: T-012` commit trailers); bupha is the CI repo again — the
  org gate is mid-migration back from the peregrin `ci-gate`
  campaign (PL-013).
- **mycena** — surfaces TODO/FIXME in code → `myco absorb`.
- **sepia** — session lifecycle → `myco handoff` / `myco resume`.

If a feature needs a daemon, a multi-machine view, or arbitration
between actors, it belongs to peregrin, not here.

## Commands

```bash
cargo xtask check          # fmt + clippy -D warnings
cargo xtask test           # whole suite (nextest if present)
cargo xtask pack           # stage libraries/myco driver payload
                           # (release tamias-driver-myco → drivers/bin/<triple>/)
cargo xtask install        # release-build tamias-driver-myco →
                           # ~/.cargo/bin; the `myco` command itself is
                           # the tamias pack's generated bins[] shim —
                           # `cargo xtask pack` + `tamias install
                           # --allow-drivers ./libraries/myco`
cargo xtask coverage       # llvm-cov report (HTML → target/llvm-cov/html);
                           # extra args pass through, e.g. --fail-under-lines 100
cargo xtask site           # build the docs site (needs docs/ worktree)
```

Git hooks live in `.hooks/` (`core.hooksPath` = `.hooks`):
pre-commit runs `cargo fmt --check`, pre-push `cargo xtask check`.

## The worktree anatomy

`project/` and `docs/` are git worktrees of orphan branches —
`project` (the PM ledger, created by `myco init`), `docs` (the
VitePress site → GitHub Pages). Both are gitignored on `main`; never
commit their contents to `main`.

## Rules

- Workspace lints are shared (`Cargo.toml [workspace.lints]`):
  `unsafe_code` forbidden, `unwrap_used` warned — `-D warnings` in CI
  means no warnings tolerated. Tests `#![allow]` liberally.
- No `unsafe`. Edition 2021.
- Keep the binary dependency-light and sync; no async runtime.
- Git access = the shared `lasius-vcs` port (`myco-ops::vcs`, git
  CLI impl) — no git2/gix, no per-repo git crate.
- Markdown in `project/` is the file spec — deterministic output,
  stable ordering, minimal diffs; golden fixtures are frozen output.
- Agents never hand-edit `project/` when a `myco` verb exists (if
  not: edit, then `myco check`).
- `.agents/skills/` is the canonical skill location;
  `skills-lock.json` pins upstream skills.

## Docs

`docs/` worktree — VitePress + Diátaxis (tutorials/how-to/reference/
explanation) → GitHub Pages at `9aia.com/myco`. `docs/gen-*.py`
renders cli/config/crate-map from source — keep `lib.rs` module docs
accurate; they feed the generated reference.
