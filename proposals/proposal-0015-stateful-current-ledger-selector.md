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

This proposal adds a user-level, mutable ledger selector managed by a new `myco current` verb group. The safe default is **`backup`** mode: repo-local discovery still wins, and the pinned ledger answers only when cwd discovery would fail. **`always`** mode is available for users who want true "current ledger" behavior. The pointer lives in the user layer of `myco.toml` (`~/.config/myco/myco.toml`, `$XDG_CONFIG_HOME` honored) — not a separate state file.

## Requirements

- **R1 — state**: `[current]` in the user-layer `myco.toml` records `ledger` + `mode`; repo `myco.toml` cannot set it.
- **R2 — targets**: `repo` (today's discovery), `org` (index-designated org ledger), a universe repo name (index ∪ lasius clones), or an absolute worktree/repo path. `repo`/`org` are reserved — `name:<repo>` is the escape hatch (P-016).
- **R3 — verbs**: `myco current list|use|show|clear`, `--json` everywhere, exposed on CLI, `serve`/`batch`, and the driver ops surface.
- **R4 — precedence**: `--dir`/`--root` always win; `MYCO_LEDGER`/`MYCO_LEDGER_SCOPE` act as a per-invocation selector; config `always` mode applies before cwd discovery; `.myco.toml` directory pointer and `backup` mode apply after local discovery and before the built-in org fallback.
- **R5 — validation**: `current use` resolves immediately and fails loudly (`org` needs an org designation, names need universe membership, paths must qualify).
- **R6 — surfaces**: CLI, `myco ui`, `serve`/`batch`, and driver `cli.<verb>`/ops manifest share the same registry + discovery; index-sourced views (`org-board`, `org-status`) are unaffected.

## Technical Design

User config (`~/.config/myco/myco.toml`, `$XDG_CONFIG_HOME` honored):

```toml
[current]
ledger = "org"        # repo | org | <universe repo name> | </abs/path> | name:<repo>
mode   = "backup"     # always | backup
```

Resolution order:

```text
--dir / --root
→ MYCO_LEDGER / MYCO_LEDGER_SCOPE (per-invocation selector)
→ config ledger when mode = always
→ cwd inside project worktree
→ repo myco.toml project_dir
→ ancestor project/ walk
→ .myco.toml directory pointer (non-repo dirs only)
→ config ledger when mode = backup
→ org fallback
→ error
```

`repo` disables the selector for that invocation (`MYCO_LEDGER=repo` ignores the config too). `MYCO_LEDGER_SCOPE` defaults to `always` when `MYCO_LEDGER` is set. Prefer storing universe repo *names* over paths so registry moves survive; canonicalize absolute paths and revalidate on every run.

Commands:

```bash
myco current list                  # repo | org | <name> | <path> targets
myco current use org               # or <repo-name> | </path> | repo
myco current use myrepo --mode always
myco current use org --here          # writes ./.myco.toml (directory pointer)
myco current show [--json]         # ledger + mode + resolved root/worktree
myco current clear                 # ledger = repo
MYCO_LEDGER=org myco status        # per-invocation selector
```

Implementation notes (per P-016 spec): dedicated user-state loader in `myco-ops` (`myco_ops::current`) reading `MYCO_CONFIG` (when set) and the user-layer file — never repo `./myco.toml`, so repo injection is impossible by construction. `current use` rewrites the canonical user-layer file via `toml_edit`, ignoring `MYCO_CONFIG` (comments/formatting preserved). Formal precedence table, target resolution, directory pointer (`.myco.toml`), and failure semantics live in P-016. Discovery gains one stage reusing `discover::qualifies`, `owns_project`, `universe::repos` (lasius clones ∪ registry), and `RepoEntry::worktree_path`; register the verbs in `VERBS` with cli+wire handlers so CLI, serve/batch, and driver ops all expose them.

## Decisions

- Verb group: `current` — `myco current list|use|show|clear`.
- Config: `[current] ledger/mode` in user-layer `myco.toml`; repo layer cannot set it (P-016).
- Names: resolve through the index universe, which already includes lasius clones inside a workspace.
- Env: `MYCO_LEDGER` + `MYCO_LEDGER_SCOPE` as a per-invocation selector.
- Surface: all verbs on CLI, serve/batch, and driver ops.
- Workspace (`[workspace]` globs + `myco workspace` verbs) is spec'd in P-016 — multi-ledger *operations* are follow-up work, not part of this selector.
