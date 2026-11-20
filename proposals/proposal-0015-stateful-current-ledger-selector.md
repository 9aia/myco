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

This proposal adds a user-level, mutable ledger selector managed by a new `myco ledger` verb group. The safe default is a **fallback** target: repo-local discovery still wins, and the pinned ledger answers only when cwd discovery would fail. An explicit `override` scope is available for users who want true "current ledger" behavior. The pointer lives in the user layer of `myco.toml` (`~/.config/myco/myco.toml`, `$XDG_CONFIG_HOME` honored) — not a separate state file.

## Requirements

- **R1 — state**: `[ledger]` in the user-layer `myco.toml` records `target` + `scope`; repo `myco.toml` cannot set it.
- **R2 — targets**: `auto` (today's discovery), `org` (index-designated org ledger), an index-universe repo name, or an absolute worktree/repo path.
- **R3 — verbs**: `myco ledger list|use|show|clear`, `--json` everywhere, CLI-first (wire can come later).
- **R4 — precedence**: `--dir`/`--root` always win; `override` scope applies before cwd discovery; `fallback` scope applies after local discovery and before the built-in org fallback.
- **R5 — validation**: `ledger use` resolves immediately and fails loudly (`org` needs an org designation, names need index membership, paths must qualify).
- **R6 — surfaces**: CLI and `myco ui` inherit it through shared discovery; index-sourced views (`org-board`, `org-status`) are unaffected.

## Technical Design

User config (`~/.config/myco/myco.toml`, `$XDG_CONFIG_HOME` honored):

```toml
[ledger]
target = "org"        # auto | org | <index repo name> | </abs/path>
scope  = "fallback"   # fallback | override
```

Resolution order:

```text
--dir / --root
→ state target when scope = override
→ cwd inside project worktree
→ repo myco.toml project_dir
→ ancestor project/ walk
→ state target when scope = fallback
→ org fallback
→ error
```

`auto` disables the state target. Prefer storing index repo *names* over paths so registry moves survive; canonicalize absolute paths and revalidate on every run.

Commands:

```bash
myco ledger list                  # auto/org/index repos with ledgers
myco ledger use org               # or <repo-name> | </path> | auto
myco ledger use myrepo --scope override
myco ledger show [--json]         # target + scope + resolved root/worktree
myco ledger clear                 # target = auto
```

Implementation notes: add `LedgerConfig` to `myco_core::Config` but merge `[ledger]` from the user layer only — repo `myco.toml` must not pin the global ledger. `ledger use` rewrites the resolved user-layer `myco.toml` via `toml_edit` (comments/formatting preserved) and ignores `MYCO_CONFIG` for the write target. Discovery gains one stage reusing `discover::qualifies`, `owns_project`, `universe::repos`, and `RepoEntry::worktree_path`.

## Open questions

- Group name: `ledger` vs `use`/`current`.
- Should `<name>` resolution require the index universe, or also scan lasius clones directly?
- Do we want `MYCO_LEDGER=<target>` as a per-invocation env override, or wait until the config proves useful?
- Should `ledger show` be exposed on `serve`/`batch`, or stay CLI-only?
