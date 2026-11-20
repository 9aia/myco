+++
id = "P-016"
title = "Ledger-selection model: formal precedence spec and naming"
status = "done"
created = "2026-11-20"
priority = "med"
links = ["T-090", "P-015"]
done = "2026-11-20"
+++

# Ledger-selection model: scenarios, precedence, workspace

Status: **draft** · Created: 2026-11-20

## Overview

Myco's ledger selection was prose-level: precedence order, target vocabulary, and the relationship between repo ledger / index / org designation / current were not formally specified. This proposal defines the model **scenario-first** — the use cases users actually have — plus the formal precedence order, target vocabulary, config surfaces, and the *workspace* (multi-repo glob selection) that sits alongside the single-ledger selector. P-015 implements the selector; P-017 documents the model.

## Scenarios

1. **Repo auto-detect** (default, most common): open a myco-inited repo → its own `project/` ledger. No config needed.
2. **Plain-folder pointer**: cd into a non-repo folder (or under one) containing `.myco.toml` declaring a ledger → that ledger. "I'm in another folder — open the org ledger here."
3. **Global pin**: `[current]` in user `myco.toml` — `always` (this ledger wins over repo discovery) or `backup` (only when discovery finds nothing).
4. **Per-invocation**: `--dir`, `--root`, `MYCO_LEDGER` — beat everything, every time.
5. **Workspace**: operate on a *set* of repos chosen by glob — multi-ledger scope; the index derives paths, worktrees, and ledger modes automatically.
6. **Org fallback**: last resort before error.

## Requirements

- **R1 — precedence spec**: formal ordered resolution table covering `--dir`/`--root`, `MYCO_LEDGER`/`MYCO_LEDGER_SCOPE`, `[current]` modes, cwd discovery, `.myco.toml` pointer, org fallback, error.
- **R2 — vocabulary**: canonical definitions for repo ledger, index, universe, org designation, current selector, directory pointer, workspace.
- **R3 — config surfaces**: `[current]` + `[workspace]` in user-layer `myco.toml`; `.myco.toml` per-directory; repo `./myco.toml` can set none of them.
- **R4 — failure semantics**: stale targets hard-error in every mode; escape hatch documented.
- **R5 — workspace resolution**: globs over universe names; expansion, validation, empty-match behavior.

## Spec

### Vocabulary

- **repo ledger** (local ledger): a repo's own `project/` worktree — orphan `project` branch content. Located by discovery (`myco.toml project_dir` or the `project/` convention).
- **ledger-less repo**: index entry with `ledger = "none"` — never resolves a ledger, including the org fallback.
- **index**: `repos.toml` under `$MYCO_HOME` (`~/.local/share/myco`) — the registry of repos myco knows; a disposable derived cache.
- **universe**: lasius workspace clones ∪ index registry entries — the named set targets, pointers, and workspace globs resolve against.
- **org designation**: the single index entry with `ledger = "org"`; its worktree is the org-wide ledger.
- **current selector**: the user-layer `[current]` table — a sticky ledger choice + mode.
- **directory pointer**: a `.myco.toml` file in a plain (non-repo) directory, found by upward search — a per-location ledger declaration.
- **workspace**: a named set of universe repos selected by glob — the multi-ledger scope.
- **explicit override**: per-invocation selectors — `--dir`, `--root`, `MYCO_LEDGER`.

### Naming

`current` is the canonical name for the single-ledger selector: verb group `myco current …`, config `[current]`, env `MYCO_LEDGER`/`MYCO_LEDGER_SCOPE`. Rationale: `ledger` already means `LedgerMode` (`local|org|none`) in the index — overloading it would collide. `workspace` names the multi-repo set selector.

Config shapes (user layer only):

```toml
# ~/.config/myco/myco.toml
[current]
ledger = "org"        # repo | org | <universe repo name> | </abs/path> | name:<repo>
mode   = "backup"     # always | backup

[workspace]
repos = ["9aia/*", "myco"]   # globs over universe names
```

(`ledger`, not `target` — the key names what is being pointed at.)

### Targets

- `repo` — selector inert; reserved word. `org` likewise reserved.
- `name:<repo>` — escape hatch forcing universe-name lookup, bypassing reserved words.
- `org` — requires exactly one index entry with `ledger = "org"`; missing → unresolvable.
- `<name>` — universe lookup; exactly one match required, zero/multiple → unresolvable (error lists candidates).
- `</abs/path>` — absolute only; a ledger worktree directly, or a repo root resolved like `--root`. Revalidated every invocation.

### Modes (single-ledger selector)

- `always` — the pinned ledger wins over cwd discovery (true "current ledger"). Applied at precedence row 4.
- `backup` — the pin answers only when cwd discovery finds nothing (safe default). Applied at row 9, after the directory pointer.

### Precedence — formal resolution order

First match wins. `D` = discovery stage.

| # | Source | Kind | Applies when | Yields |
|---|--------|------|--------------|--------|
| 1 | `--dir <PATH>` | explicit CLI | always | the worktree path directly |
| 2 | `--root <PATH>` | explicit CLI | always (loses to `--dir`) | `<root>/project` or repo `project_dir` |
| 3 | `MYCO_LEDGER=<target>` | env, per-invocation | target ≠ `repo` | resolved target; unresolvable → hard error |
| 4 | `[current] mode="always"` | persistent user | `ledger` ≠ `repo` | resolved target; unresolvable → hard error |
| 5 | cwd inside a `project/` worktree | D | cwd resolves to a ledger worktree | that worktree |
| 6 | repo `myco.toml project_dir` | D | cwd inside a repo root | `<repo>/<project_dir>` |
| 7 | ancestor `project/` walk | D | qualifying `project/` found upward | that worktree |
| 8 | `.myco.toml` directory pointer | D (upward) | nearest pointer found, not inside a git repo | declared ledger |
| 9 | `[current] mode="backup"` | persistent user | `ledger` ≠ `repo` | resolved target; unresolvable → hard error |
| 10 | org fallback | index | an org designation exists | org worktree |
| 11 | — | — | nothing matched | error |

Notes:

- `MYCO_LEDGER_SCOPE` defaults to `always` when `MYCO_LEDGER` is set; `MYCO_LEDGER=repo` disables the env override entirely.
- `repo` as `[current].ledger` disables the selector — rows 4 and 9 never match.
- Rows 5–7 only match when they yield a ledger; a ledger-less repo context falls through.
- Row 8 sits *after* repo discovery deliberately: a repo's own ledger always beats a pointer file — pointers serve plain folders, not repos.
- Row 4 (`always`) beats row 8: an explicit global pin is stronger than a directory hint. Escape per-invocation via row 3.
- Row 10 keeps existing semantics: last resort before error, available whenever the index is readable — including outside any repo. `ledger = "none"` repos opt out (verify during P-015).

### Directory pointer (`.myco.toml`)

- A dotfile in a plain directory: `ledger = "org"` (same target vocabulary as `[current]`).
- Found by upward search from cwd; the **nearest** `.myco.toml` wins.
- **Ignored inside git repositories** — a committed pointer would be repo-controlled injection, same class as repo `myco.toml` setting `[current]`. Pointers serve non-repo folders only.
- Fallback semantics only (row 8): no mode/scope — a pointer never beats a repo's own ledger.
- Writes: `myco current use <target> --here` creates/updates the cwd `.myco.toml`; `myco current clear --here` removes it (nearest-or-cwd file) via `toml_edit`. `--here` selects the directory-pointer store instead of the user config — same verb group, no separate `pointer` verbs. `current show` needs no flag: it reports whatever discovery resolves.

### Workspace (multi-repo)

- `[workspace] repos` in user `myco.toml`: glob patterns matched against **universe repo names** (lasius clones ∪ registry). `*`/`?`/`[]` semantics per the `glob` crate conventions.
- Resolution produces the set of matching `RepoEntry`s — paths, worktrees, ledger modes come from the index, never duplicated in config.
- Zero matches for the whole set → error at use time; individual non-matching globs warn (a typo'd glob shouldn't silently shrink the workspace).
- `myco workspace list|use|show|clear` mirrors the `current` verbs (`--json` everywhere); `use` writes `[workspace]` via `toml_edit`.
- Consumers: index-sourced views (`org-board`, `org-status`) and future multi-ledger verbs. The workspace is a *scope*, not a ledger — single-ledger verbs keep using `[current]`/discovery.

### Config mechanics

- `[current]` and `[workspace]` are read from `MYCO_CONFIG` (when set — it wins) and the user-layer file `$XDG_CONFIG_HOME/myco/myco.toml` / `~/.config/myco/myco.toml`. Never from repo `./myco.toml`.
- `myco current|workspace use|clear` always write the canonical user-layer file via `toml_edit`, ignoring `MYCO_CONFIG` — env is a per-invocation read override; persistence lives in the user file.
- `.myco.toml` is a standalone single-key file, not part of the layered config merge.
- Implementation: dedicated user-state loader in `myco-ops` (e.g. `myco_ops::current`) — not fields on `myco_core::Config`'s layered merge, so repo-layer exclusion is structural.

### Failure semantics

| Situation | any mode / env / `current use` |
|-----------|-------------------------------|
| target unresolvable at runtime | hard error naming target + escape hint — stale config is never silent |
| `current use <bad>` | fails immediately, writes nothing |
| `current show` | reports resolved worktree, or the stale reason |
| workspace glob matches nothing | error listing the globs and the universe names tried |

Escape hatch: a stale `always`-mode or env target blocks *every* verb — including `current` itself. The error must hint `MYCO_LEDGER=repo myco current clear` (env row 3 beats persisted row 4), not bare `myco current clear`, which would hit the same wall. `--dir`/`--root` work too but require knowing a valid path.

### Surfaces

`myco current list|use|show|clear` and `myco workspace list|use|show|clear` (`--json` everywhere) are registered verbs exposed identically on CLI, `serve`, `batch`, and the driver ops manifest. All verbs go through the same ledger discovery — uniform rule, no special-casing. CLI, TUI, serve, and batch share `myco-ops` discovery, so the selector applies uniformly. Index-sourced views bypass single-ledger selection by design — they name their source (or the workspace) explicitly.

## Decisions

- Scenarios are the organizing frame; the precedence table is the formalization underneath.
- Modes: `always`|`backup` — confirmed. `fallback` stays reserved for the pointer/org rows to avoid a third "fallback".
- Directory pointer: `.myco.toml`, upward search, non-repo dirs only, fallback semantics only.
- Workspace = `[workspace] repos` globs over universe names; index derives the rest. Multi-ledger *operations* remain future work — this spec covers selection only.
- Naming: `current` for the single-ledger selector; `workspace` for the repo set.
- `ledger = "none"` opts out of org fallback (verify against current impl during P-015).
- Feeds P-015 implementation and P-017 docs; landing this first is recommended.
