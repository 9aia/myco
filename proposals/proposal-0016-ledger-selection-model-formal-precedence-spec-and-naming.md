+++
id = "P-016"
title = "Ledger-selection model: formal precedence spec and naming"
status = "open"
created = "2026-11-20"
priority = "med"
links = ["T-090", "P-015"]
+++

## Overview

P-015 adds a persistent current-ledger selector, but the selection model is still prose-level: precedence order, target vocabulary, and the relationship between repo ledger / index / org designation / current are not formally specified. This proposal defines that model so P-015 (and docs, T-089) implement against a spec instead of re-deriving it.

## Requirements

- **R1 — precedence spec**: a formal, ordered resolution table covering `--dir`/`--root`, `MYCO_LEDGER`/`MYCO_LEDGER_SCOPE`, config `[current]` scope=fallback|override, cwd worktree discovery, `project_dir`, ancestor walk, org fallback, error. Each row: source, when it applies, what it can override.
- **R2 — vocabulary**: canonical definitions for *repo ledger* (a repo's own `project/` worktree), *index* (`repos.toml` registry), *universe* (lasius clones ∪ registry), *org designation* (`ledger="org"`), *current selector* (user-layer `[current]`). Naming review: is `current` the right verb/config key vs `ledger`?
- **R3 — config surface**: decide whether `[current]` lives in `myco_core::Config` with user-layer-only merge, or a dedicated user-state loader; document why repo-layer injection must be impossible.
- **R4 — failure semantics**: what happens when a persisted target goes stale (repo moved, org undesignated) — hard error vs warn-and-fallback, per scope.

## Spec

### Vocabulary

- **repo ledger** (local ledger): a repo's own `project/` worktree — orphan `project` branch content (TODO.md, issues/, proposals/, …). Located by discovery (`myco.toml project_dir` or the `project/` convention).
- **ledger-less repo**: index entry with `ledger = "none"` — never resolves a ledger, including the org fallback.
- **index**: `repos.toml` under `$MYCO_HOME` (`~/.local/share/myco`) — the registry of repos myco knows; a disposable derived cache.
- **universe**: lasius workspace clones ∪ index registry entries — the named set `current` targets and `org-*` views resolve against.
- **org designation**: the single index entry with `ledger = "org"`; its worktree is the org-wide ledger.
- **current selector**: the user-layer `[current]` table — a sticky `ledger` target + `scope`.
- **explicit override**: per-invocation selectors — `--dir`, `--root`, `MYCO_LEDGER`.

### Naming

`current` is the canonical name: verb group `myco current …`, config `[current]`, env `MYCO_LEDGER`/`MYCO_LEDGER_SCOPE`. Rationale: `ledger` already means `LedgerMode` (`local|org|none`) in the index — overloading it for the user pointer would collide. `current` names the pointer ("which ledger is current"), not the ledger kind.

Config shape (user layer only):

```toml
[current]
ledger = "org"        # auto | org | <universe repo name> | </abs/path>
scope  = "fallback"   # fallback | override
```

(`ledger`, not `target` — the key names what is being pointed at.)

### Precedence — formal resolution order

First match wins. `D` = discovery stage.

| # | Source | Kind | Applies when | Yields |
|---|--------|------|--------------|--------|
| 1 | `--dir <PATH>` | explicit CLI | always | the worktree path directly |
| 2 | `--root <PATH>` | explicit CLI | always (loses to `--dir` if both) | `<root>/project` or repo `project_dir` |
| 3 | `MYCO_LEDGER=<target>` | env, per-invocation | target ≠ `auto` | resolved target; unresolvable → hard error |
| 4 | `[current] scope="override"` | persistent user | `ledger` ≠ `auto` | resolved target; unresolvable → hard error |
| 5 | cwd inside a `project/` worktree | D | cwd resolves to a ledger worktree | that worktree |
| 6 | repo `myco.toml project_dir` | D | cwd inside a repo root (not the worktree) | `<repo>/<project_dir>` |
| 7 | ancestor `project/` walk | D | qualifying `project/` found upward | that worktree |
| 8 | `[current] scope="fallback"` | persistent user | `ledger` ≠ `auto` | resolved target; unresolvable → warn, continue |
| 9 | org fallback | index | an org designation exists | org worktree |
| 10 | — | — | nothing matched | error |

Notes:

- `MYCO_LEDGER_SCOPE` defaults to `override` when `MYCO_LEDGER` is set; `MYCO_LEDGER=auto` disables the env override entirely.
- `auto` in `[current]` disables the selector — rows 4 and 8 never match.
- Rows 5–7 are today's discovery, unchanged and indivisible: cwd context always beats a fallback-scoped selector.
- Row 9 keeps existing semantics: it is the last resort before error, available whenever the index is readable — including outside any repo. `ledger = "none"` repos opt out.

### Target resolution

- `auto` — selector inert.
- `org` — requires exactly one index entry with `ledger = "org"`; missing → unresolvable.
- `<name>` — universe lookup (lasius clones ∪ registry). Name must match exactly one entry; zero → unresolvable, multiple → unresolvable (ambiguous, error lists candidates). Registry entries win over lasius clones on collision only if names are identical *and* paths agree; otherwise ambiguous.
- `</abs/path>` — must be absolute. Accepts a ledger worktree directly, or a repo root resolved by the same rules as `--root`. Validated every invocation via `discover::qualifies`/`owns_project`.

### Config mechanics

- `[current]` is read from and written to **only** the user-layer file: `$XDG_CONFIG_HOME/myco/myco.toml`, else `~/.config/myco/myco.toml`.
- It is **never** read from `MYCO_CONFIG` or repo `./myco.toml`, and repo-layer `[current]` is ignored even if present — a repo must not pin the user's global ledger.
- Implementation: dedicated user-state loader in `myco-ops` (e.g. `myco_ops::current`) reading that single file — not a field on `myco_core::Config`'s layered merge, so the invariant is structural rather than a merge policy.
- `myco current use|clear` writes via `toml_edit` (comments/formatting preserved); `MYCO_CONFIG` is ignored for the write destination.

### Failure semantics

| Situation | scope=override / env / `current use` | scope=fallback |
|-----------|--------------------------------------|----------------|
| target unresolvable at runtime | hard error naming target + `myco current clear` hint | stderr warning, continue to next row |
| `current use <bad>` | fails immediately, writes nothing | — |
| `current show` | reports resolved worktree, or the stale reason | same |

### Surfaces

`myco current list|use|show|clear` (`--json` everywhere) are registered verbs exposed identically on CLI, `serve`, `batch`, and the driver ops manifest. CLI, TUI, serve, and batch all share `myco-ops` discovery, so the selector applies uniformly. Index-sourced views (`org-board`, `org-status`) bypass selection by design — they name their source explicitly.

## Decisions

- Naming: `current` — approved. P-015 should be updated from `[ledger] target` to `[current] ledger`.
- Config: dedicated user-state loader, not `Config` merge — repo injection impossible by construction.
- `ledger = "none"` opts out of org fallback (verify against current impl during P-015).
- Feeds P-015 implementation and P-017 docs; landing this first is recommended.
