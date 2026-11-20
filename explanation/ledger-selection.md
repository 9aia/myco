---
url: /myco/explanation/ledger-selection.md
---
# Ledger selection — which ledger does `myco` open?

Myco can see many ledgers at once: every repo in the workspace has
its own `project/` worktree, the index knows all of them, and one of
them is designated the *org* ledger. This page explains how myco
chooses which one a given invocation reads and writes — the mental
model first, then the formal precedence table.

## The layers

Four separate ideas are easy to conflate. Keep them apart:

* **Repo ledger** — a repository's own `project/` worktree. When you
  `cd` into a myco-initialized repo, this is the ledger myco opens.
  Nothing needs configuring; discovery finds it.
* **Index / universe** — `repos.toml` under `$MYCO_HOME` is a
  disposable registry of known repos; the *universe* is lasius
  workspace clones ∪ registry entries. It is an address book, not
  data — delete it and myco loses nothing canonical.
* **Org designation** — one universe entry marked `ledger = "org"`
  (`[index] ledger = "org"` in its `myco.toml`, or
  `myco index add <repo> --org`). Its worktree is the fallback
  ledger for ledger-less repos and the last resort before error.
* **Current selector** — *your* sticky pointer in user-layer
  `~/.config/myco/myco.toml` (`[current]`). It answers "which
  ledger" when cwd alone doesn't say — from another repo, a plain
  folder, or anywhere at all.

Two more surfaces sit alongside:

* **Directory pointer** — a `.myco.toml` file in a plain (non-git)
  folder saying `ledger = "…"`. Found by walking up from cwd;
  ignored inside git repos, where it would be repo-controlled
  injection. Fallback semantics only — a repo's own ledger always
  beats it.
* **Workspace** — `[workspace] repos` glob patterns in user config
  selecting a *set* of universe repos. It scopes index-sourced
  views (`org-board`, `org-status`); it is not a ledger and does
  not affect single-ledger verbs.

## Scenarios

| You are… | What myco does |
|---|---|
| Inside a repo with its own `project/` | Opens that repo's ledger — the default, no config needed. |
| Inside a ledger-less repo | Falls through to the org-designated ledger. |
| In a plain folder with a `.myco.toml` pointer | Opens the pointer's target — but only if nothing closer claimed the cwd first. |
| Anywhere, with `[current]` set | `mode = "always"` pins that ledger everywhere; `mode = "backup"` answers only when discovery finds nothing (still beating the org fallback). |
| Passing `--dir`, `--root`, or `MYCO_LEDGER` | That explicit target wins over everything, every time. |
| Running `org-board` / `org-status` | Reads the index universe — filtered to `[workspace]` when configured. Bypasses ledger selection entirely. |

## Precedence — the formal order

First match wins. Rows 1–2 are explicit per-invocation flags; rows
3–4 are selectors that answer *before* looking at where you are;
rows 5–7 are ordinary cwd discovery; rows 8–9 are late selectors;
row 10 is the org fallback.

| # | Source | Applies when | Yields |
|---|--------|--------------|--------|
| 1 | `--dir <PATH>` | always | the worktree path directly |
| 2 | `--root <PATH>` | always (loses to `--dir`) | `<root>/project` or repo `project_dir` |
| 3 | `MYCO_LEDGER=<target>` | target ≠ `repo` | resolved target; unresolvable → hard error |
| 4 | `[current] mode="always"` | `ledger` ≠ `repo` | resolved target; unresolvable → hard error |
| 5 | cwd inside a `project/` worktree | cwd resolves to a ledger | that worktree |
| 6 | repo `myco.toml project_dir` | cwd inside a repo | `<repo>/<project_dir>` |
| 7 | ancestor `project/` walk | qualifying `project/` upward | that worktree |
| 8 | `.myco.toml` directory pointer | nearest pointer, not inside a git repo | declared ledger |
| 9 | `[current] mode="backup"` (+ env `MYCO_LEDGER_SCOPE=backup`) | `ledger` ≠ `repo` | resolved target; unresolvable → hard error |
| 10 | org fallback | an org designation exists | org worktree |
| 11 | — | nothing matched | error |

Notes that matter:

* `MYCO_LEDGER_SCOPE` defaults to `always` when `MYCO_LEDGER` is
  set; `MYCO_LEDGER=repo` disables the env override entirely.
* Rows 5–7 only match when they yield a ledger — a ledger-less repo
  context falls through.
* Row 8 sits after repo discovery deliberately: a repo's own ledger
  always beats a pointer file.
* Row 4 (`always`) beats row 8: an explicit global pin is stronger
  than a directory hint.
* **Stale targets hard-error in every mode** — a moved repo or
  removed org designation fails loudly rather than silently
  falling back. The escape hatch is `MYCO_LEDGER=repo <cmd>` (env
  row 3 beats the persisted row 4, so `myco current clear` itself
  would hit the same wall).

## Targets

`[current].ledger`, `MYCO_LEDGER`, pointer files, and
`current use`/`workspace`-adjacent resolution share one vocabulary:

* `repo` — disable the selector (the inert default).
* `org` — the index-designated org ledger.
* `<name>` — a universe repo name (lasius clone or registry entry).
* `</abs/path>` — a ledger worktree directly, or a repo root
  resolved like `--root`.
* `name:<repo>` — forces universe-name lookup, disambiguating
  repos literally named `repo` or `org`.

Ambiguous names hard-error listing the candidates.

## Surfaces

CLI, `myco ui`, `serve`, `batch`, and the driver's `cli.<verb>`
queries all share `myco-ops` discovery — the selector applies
uniformly, `--json` everywhere. The exceptions are deliberate:
index-sourced views (`org-board`, `org-status`) name their source
explicitly and bypass single-ledger selection; `[workspace]`
scopes them instead. Index bookkeeping verbs (`index add/remove`)
also stay strict — a pinned ledger must never hijack repo
registration.
