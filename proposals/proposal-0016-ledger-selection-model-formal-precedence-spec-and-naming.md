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

- **R1 — precedence spec**: a formal, ordered resolution table covering `--dir`/`--root`, `MYCO_LEDGER`/`MYCO_LEDGER_SCOPE`, config `[current]` scope=first|last, cwd worktree discovery, `project_dir`, ancestor walk, org fallback, error. Each row: source, when it applies, what it can override.
- **R2 — vocabulary**: canonical definitions for *repo ledger* (a repo's own `project/` worktree), *index* (`repos.toml` registry), *universe* (lasius clones ∪ registry), *org designation* (`ledger="org"`), *current selector* (user-layer `[current]`). Naming review: is `current` the right verb/config key vs `ledger`?
- **R3 — config surface**: decide whether `[current]` lives in `myco_core::Config` with user-layer-only merge, or a dedicated user-state loader; document why repo-layer injection must be impossible.
- **R4 — failure semantics**: what happens when a persisted target goes stale (repo moved, org undesignated) — decided: hard error for every scope.

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
ledger = "org"        # repo | org | <universe repo name> | </abs/path>
scope  = "last"      # first | last
```

(`ledger`, not `target` — the key names what is being pointed at.)

### Precedence — formal resolution order

First match wins. `D` = discovery stage.

| # | Source | Kind | Applies when | Yields |
|---|--------|------|--------------|--------|
| 1 | `--dir <PATH>` | explicit CLI | always | the worktree path directly |
| 2 | `--root <PATH>` | explicit CLI | always (loses to `--dir` if both) | `<root>/project` or repo `project_dir` |
| 3 | `MYCO_LEDGER=<target>` | env, per-invocation | target ≠ `repo` | resolved target; unresolvable → hard error |
| 4 | `[current] scope="first"` | persistent user | `ledger` ≠ `repo` | resolved target; unresolvable → hard error |
| 5 | cwd inside a `project/` worktree | D | cwd resolves to a ledger worktree | that worktree |
| 6 | repo `myco.toml project_dir` | D | cwd inside a repo root (not the worktree) | `<repo>/<project_dir>` |
| 7 | ancestor `project/` walk | D | qualifying `project/` found upward | that worktree |
| 8 | `[current] scope="last"` | persistent user | `ledger` ≠ `repo` | resolved target; unresolvable → hard error |
| 9 | org fallback | index | an org designation exists | org worktree |
| 10 | — | — | nothing matched | error |

Notes:

- `MYCO_LEDGER_SCOPE` defaults to `first` when `MYCO_LEDGER` is set; `MYCO_LEDGER=repo` disables the env override entirely.
- `repo` in `[current]` disables the selector — rows 4 and 8 never match.
- Rows 5–7 are today's discovery, unchanged and indivisible: cwd context always beats a `last`-scoped selector.
- Row 9 keeps existing semantics: it is the last resort before error, available whenever the index is readable — including outside any repo. `ledger = "none"` repos opt out.

### Target resolution

- `repo` — selector inert; reserved word, so a universe repo named `repo` cannot be targeted by bare name — use `name:repo` or its path. `org` is likewise reserved.
- `name:<repo>` — escape hatch forcing universe-name lookup, bypassing the reserved words. The only way to target a repo named `repo`/`org` by name.
- `org` — requires exactly one index entry with `ledger = "org"`; missing → unresolvable.
- `<name>` — universe lookup (lasius clones ∪ registry). Name must match exactly one entry; zero → unresolvable, multiple → unresolvable (ambiguous, error lists candidates). Registry entries win over lasius clones on collision only if names are identical *and* paths agree; otherwise ambiguous.
- `</abs/path>` — must be absolute. Accepts a ledger worktree directly, or a repo root resolved by the same rules as `--root`. Validated every invocation via `discover::qualifies`/`owns_project`.

### Config mechanics

- `[current]` is read from `MYCO_CONFIG` (when set — it wins) and the user-layer file `$XDG_CONFIG_HOME/myco/myco.toml` / `~/.config/myco/myco.toml`. It is **never** read from repo `./myco.toml` — a repo must not pin the user's global ledger.
- `myco current use|clear` always writes the canonical user-layer file via `toml_edit` (comments/formatting preserved), ignoring `MYCO_CONFIG` — env is a per-invocation read override, persistence lives in the user file.
- Implementation: dedicated user-state loader in `myco-ops` (e.g. `myco_ops::current`) — not a field on `myco_core::Config`'s layered merge, so the repo-layer exclusion is structural rather than a merge policy.

### Failure semantics

| Situation | any scope / env / `current use` |
|-----------|-------------------------------|
| target unresolvable at runtime | hard error naming target + `myco current clear` hint — stale config is never silent |
| `current use <bad>` | fails immediately, writes nothing |
| `current show` | reports resolved worktree, or the stale reason |

### Surfaces

`myco current list|use|show|clear` (`--json` everywhere) are registered verbs exposed identically on CLI, `serve`, `batch`, and the driver ops manifest. `current list` shows every ledger-capable universe repo plus the `org` and `repo` targets. The `current` verbs go through the same ledger discovery as every other verb — uniform rule, no special-casing. CLI, TUI, serve, and batch all share `myco-ops` discovery, so the selector applies uniformly. Index-sourced views (`org-board`, `org-status`) bypass selection by design — they name their source explicitly.

## Decisions

- Naming: `current` — approved. Target `repo` replaces `auto` (reserved word; `name:<repo>` is the disambiguation escape hatch).
- Scopes renamed `fallback|override` → `last|first` — positional names matching the resolution order.
- Config: dedicated user-state loader, not `Config` merge — repo injection impossible by construction. `MYCO_CONFIG` may *read* `[current]` (wins over user file); writes always hit the canonical user file.
- Stale targets hard-error in every scope — no silent warn-and-continue.
- `ledger = "none"` opts out of org fallback (verify against current impl during P-015).
- Feeds P-015 implementation and P-017 docs; landing this first is recommended.
