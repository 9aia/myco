+++
id = "P-015"
title = "Stateful current-ledger selector"
status = "open"
created = "2026-11-20"
priority = "med"
+++

# Stateful current-ledger selector

Status: **draft** · Created: 2026-11-20

## Goal & Overview

Today myco picks a ledger per invocation: `--dir`/`--root`, cwd discovery, repo `project_dir`, ancestor walk, then the org-ledger fallback. There is no sticky "current ledger" — from another repo or folder there is no way to say "use that one" without repeating flags or a shell alias.

This proposal adds a user-level, mutable ledger selector managed by a new `myco current` verb group. The safe default is a **fallback** target: repo-local discovery still wins, and the pinned ledger answers only when cwd discovery would fail. An explicit `override` scope is available for users who want true "current ledger" behavior. The pointer lives in the user layer of `myco.toml` (`~/.config/myco/myco.toml`, `$XDG_CONFIG_HOME` honored) — not a separate state file.

## Requirements

- **R1 — state**: `[current]` in the user-layer `myco.toml` records `target` + `scope`; repo `myco.toml` cannot set it.
- **R2 — targets**: `auto` (today's discovery), `org` (index-designated org ledger), a universe repo name (index ∪ lasius clones), or an absolute worktree/repo path.
- **R3 — verbs**: `myco current list|use|show|clear`, `--json` everywhere, exposed on CLI, `serve`/`batch`, and the driver ops surface.
- **R4 — precedence**: `--dir`/`--root` always win; `MYCO_LEDGER`/`MYCO_LEDGER_SCOPE` act as a per-invocation override; config `override` scope applies before cwd discovery; `fallback` scope applies after local discovery and before the built-in org fallback.
- **R5 — validation**: `current use` resolves immediately and fails loudly (`org` needs an org designation, names need universe membership, paths must qualify).
- **R6 — surfaces**: CLI, `myco ui`, `serve`/`batch`, and driver `cli.<verb>`/ops manifest share the same registry + discovery; index-sourced views (`org-board`, `org-status`) are unaffected.

## Technical Design

User config (`~/.config/myco/myco.toml`, `$XDG_CONFIG_HOME` honored):

```toml
[current]
target = "org"        # auto | org | <universe repo name> | </abs/path>
scope  = "fallback"   # fallback | override
```

Resolution order:

```text
--dir / --root
→ MYCO_LEDGER / MYCO_LEDGER_SCOPE (per-invocation override)
→ config target when scope = override
→ cwd inside project worktree
→ repo myco.toml project_dir
→ ancestor project/ walk
→ config target when scope = fallback
→ org fallback
→ error
```

`auto` disables the selector for that invocation (`MYCO_LEDGER=auto` ignores the config too). `MYCO_LEDGER_SCOPE` defaults to `override` when `MYCO_LEDGER` is set. Prefer storing universe repo *names* over paths so registry moves survive; canonicalize absolute paths and revalidate on every run.

Commands:

```bash
myco current list                  # auto/org/universe repos with ledgers
myco current use org               # or <repo-name> | </path> | auto
myco current use myrepo --scope override
myco current show [--json]         # target + scope + resolved root/worktree
myco current clear                 # target = auto
MYCO_LEDGER=org myco status        # per-invocation override
```

Implementation notes: add `CurrentConfig` to `myco_core::Config` but merge `[current]` from the user layer only — repo `myco.toml` must not pin the global ledger. `current use` rewrites the resolved user-layer `myco.toml` via `toml_edit` (comments/formatting preserved) and ignores `MYCO_CONFIG` for the write target. Discovery gains one stage reusing `discover::qualifies`, `owns_project`, `universe::repos` (lasius clones ∪ registry), and `RepoEntry::worktree_path`; register the verbs in `VERBS` with cli+wire handlers so CLI, serve/batch, and driver ops all expose them.

## Decisions

- Verb group: `current` — `myco current list|use|show|clear`.
- Config: `[current] target/scope` in user-layer `myco.toml`; repo layer cannot set it.
- Names: resolve through the index universe, which already includes lasius clones inside a workspace.
- Env: `MYCO_LEDGER` + `MYCO_LEDGER_SCOPE` as a per-invocation override.
- Surface: all verbs on CLI, serve/batch, and driver ops.
