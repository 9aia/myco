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

## Layout

Ports-and-adapters, flat `crates/` workspace. `myco-core` is pure
domain (no I/O); ports are traits, impls are separate crates;
`crates/myco-cli` (package `myco`) is the composition root.

| Crate | Role |
| --- | --- |
| `crates/myco-core` | Item models, ID registry, state machine, deps — pure, no I/O |
| `crates/myco-config` | Config port — where `myco.toml`-shaped config comes from |
| `crates/myco-config-toml` | Config impl — explicit path → `$MYCO_CONFIG` → `./myco.toml` → `~/.config/myco/` |
| `crates/myco-git` | Git plumbing via the `git` CLI — `project` orphan branch, worktree, auto-commit, log |
| `crates/myco-shell` | `bash -c <cmd>` in a workdir — generic shell-out |
| `crates/myco-store` | Store port — the `project/` persistence trait |
| `crates/myco-store-md` | Markdown impl — strict parse/emit; owns the file spec |
| `crates/myco-index` | Index port — the derived, disposable cache (registry + snapshots) |
| `crates/myco-index-redb` | Index impl — redb snapshots + `repos.toml` under `~/.local/share/myco` |
| `crates/myco-ops` | Ops I/O shell — discovery, store open/save, commit-per-mutation, verb plumbing; pure pieces live in core |
| `crates/myco-testkit` | Golden fixtures + conformance for the file spec |
| `crates/myco-tui` | Interactive board TUI (ratatui) — thin front-end over myco-ops; `myco ui`, feature-gated |
| `crates/myco-cli` | The `myco` binary — clap, `--json` everywhere, `manifest`/`gen-cli-docs` |
| `xtask` | check / test / coverage / install / site |

Planned (per `project/proposals/proposal-0001-*.md`): `myco-template`.

## Boundary

myco owns **work memory** — the ledger of intent (tasks, issues,
proposals, plans, ADRs, handoffs). Neighbors own theirs:

- **sturn** — ephemeral coordination (live claims/ops/topics).
- **kuda** — code memory; myco indexes *work items*, not code.
- **peregrin** — orchestration; optional `myco serve` (ndjson) is a
  later ADR.
- **bupha** — CI gate; `myco check` in pipelines, `Item: T-012`
  commit trailers.
- **mycena** — surfaces TODO/FIXME in code → `myco absorb`.
- **sepia** — session lifecycle → `myco handoff` / `myco resume`.

If a feature needs a daemon, a multi-machine view, or arbitration
between actors, it belongs to peregrin, not here.

## Commands

```bash
cargo xtask check          # fmt + clippy -D warnings
cargo xtask test           # whole suite (nextest if present)
cargo xtask install        # release-install myco → ~/.cargo/bin
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
- Git access = shell out to the `git` CLI (`myco-git`) — no git2/gix.
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
