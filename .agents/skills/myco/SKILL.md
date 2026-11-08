---
name: myco
description: Manage project state via the myco CLI — tasks, backlog, issues, proposals, plans, ADRs, handoffs in `project/` worktree files. Trigger when working with `project/` files, planning, tracking tasks, or starting/resuming work.
metadata:
  version: "2.0.0"
---

# myco — agent-first project management

`myco` owns every deterministic write to a repo's `project/` worktree
(orphan `project` branch, gitignored on main). Markdown is canonical;
the CLI is the write path; git history is the audit trail.

## The one rule

**Never hand-edit `project/` files when a `myco` verb exists.** If a
change genuinely has no verb, edit — then run `myco check` to
re-validate and `myco check --fix` to repair drift.

## Bootstrap

```bash
command -v myco   # missing? install from the repo:
cargo xtask install        # in 9aia/myco — installs to ~/.cargo/bin
```

## Discovery — prefer asking the CLI over reading docs

```bash
myco manifest          # capabilities + full verb list (JSON)
myco schema            # the file/state spec, machine-readable
myco <verb> --help     # every verb, self-documenting
```

`--json` is accepted on every verb — use it for machine consumption.
`--dir`/`--root` override worktree discovery when cwd isn't the repo.

## Verb cheat sheet — by intent

| Intent | Command |
| --- | --- |
| Session start / where things stand | `myco status --prompt` |
| Add a task | `myco todo add "title" [--pri high] [--owner devin]` |
| Park / re-activate a task | `myco todo defer T-NNN` / `myco backlog promote T-NNN` |
| Claim / start / block / finish | `myco item set T-NNN --status claimed|in-progress|blocked|done` |
| Done a task | `myco todo done T-NNN` |
| File / close an issue | `myco issue new "title"` / `myco issue resolve I-NNN` |
| Design / plan / decision docs | `myco proposal new` · `myco plan new` · `myco adr new` |
| Inspect / mutate any item | `myco item show T-NNN` · `myco item set T-NNN …` |
| Session end (resumable state) | `myco handoff T-NNN --note "done/next/open"` |
| Session died mid-work | `myco interrupt T-NNN` (→ open + interrupt handoff; ⚡ marker in status/board) |
| Restart work on an item | `myco resume T-NNN` (`item show`/`resume` take 1..n ids) |
| Pick the top item | `myco next [--claim]` — open task/issue by `pri,id`; filter flags shared with `view` |
| Views | `myco view [name]` (`board`/`open`/`epic`/`milestone` presets + `--epic`/`--milestone`/`--status`/`--group-by`/`--sort`/`--format` flags; `default_view` in myco.toml is the bare-`view` landing) · `myco report` · read-only viz via pavo (`tamias-driver-myco` → `pavo view myco:board`) |
| Config verbs | `[verbs.<name>]` in myco.toml — `emit` (a baked `ViewSpec` → table/json/ids/first) or `target`+`args` (alias for a read-only builtin); works on CLI, batch, serve |
| Served surface | `myco serve` (JSON-RPC 2.0 on stdio, for pipe-holding embedders) · `myco batch` (one-shot: NDJSON requests stdin/`--file` → NDJSON responses) |
| Cross-repo | `myco index add <repo>` (ledger-less repos register `ledger = "none"`, no snapshot; `--org` marks the one global ledger) · `myco item move <ID> --to <repo>` (re-files an item in another repo's ledger; source closes `done` + `moved to` handoff) · `myco view org-status` · `myco view org-board` · `[views.x] source="index" format="repos"` → one row per repo (org/items/open/claimed/err). In a registered ledger-less repo, project verbs resolve to the org ledger's worktree |
| Housekeeping | `myco init` · `check` · `migrate` · `commit` · `org init` |

## The daily loop

1. `myco status --prompt` — token-sized digest of the ledger.
2. Work. Track state as you go: `--status claimed` on pickup,
   `in-progress` when started, `blocked` when stuck.
3. Exit: `myco item set <ID> --status done` for finished work,
   `myco handoff <ID> --note …` for anything resumable.

## References — read only what the task needs

- [lifecycle.md](file://./references/lifecycle.md) — init / check /
  migrate / commit, the worktree model, auto-commit semantics
- [items.md](file://./references/items.md) — all item verbs, id
  scheme, state machine, metadata tokens
- [continuity.md](file://./references/continuity.md) — handoff /
  resume / status --prompt — the core agent workflow
- [views.md](file://./references/views.md) — view / report /
  index / org
