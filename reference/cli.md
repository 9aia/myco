# Command-Line Help for `myco`

This document contains the help content for the `myco` command-line program.

myco — agent-first project management for a repo's `project/` worktree.

**Command Overview:**

* [`myco`↴](#myco)
* [`myco todo`↴](#myco-todo)
* [`myco todo add`↴](#myco-todo-add)
* [`myco todo done`↴](#myco-todo-done)
* [`myco todo cancel`↴](#myco-todo-cancel)
* [`myco todo defer`↴](#myco-todo-defer)
* [`myco todo list`↴](#myco-todo-list)
* [`myco backlog`↴](#myco-backlog)
* [`myco backlog add`↴](#myco-backlog-add)
* [`myco backlog promote`↴](#myco-backlog-promote)
* [`myco backlog list`↴](#myco-backlog-list)
* [`myco done`↴](#myco-done)
* [`myco done list`↴](#myco-done-list)
* [`myco done remove`↴](#myco-done-remove)
* [`myco issue`↴](#myco-issue)
* [`myco issue new`↴](#myco-issue-new)
* [`myco issue list`↴](#myco-issue-list)
* [`myco issue resolve`↴](#myco-issue-resolve)
* [`myco issue archive`↴](#myco-issue-archive)
* [`myco issue comment`↴](#myco-issue-comment)
* [`myco proposal`↴](#myco-proposal)
* [`myco proposal new`↴](#myco-proposal-new)
* [`myco proposal list`↴](#myco-proposal-list)
* [`myco proposal status`↴](#myco-proposal-status)
* [`myco plan`↴](#myco-plan)
* [`myco plan new`↴](#myco-plan-new)
* [`myco plan list`↴](#myco-plan-list)
* [`myco adr`↴](#myco-adr)
* [`myco adr new`↴](#myco-adr-new)
* [`myco adr list`↴](#myco-adr-list)
* [`myco adr supersede`↴](#myco-adr-supersede)
* [`myco item`↴](#myco-item)
* [`myco item show`↴](#myco-item-show)
* [`myco item set`↴](#myco-item-set)
* [`myco item move`↴](#myco-item-move)
* [`myco item reorder`↴](#myco-item-reorder)
* [`myco absorb`↴](#myco-absorb)
* [`myco absorb comments`↴](#myco-absorb-comments)
* [`myco absorb jira`↴](#myco-absorb-jira)
* [`myco absorb github`↴](#myco-absorb-github)
* [`myco index`↴](#myco-index)
* [`myco index add`↴](#myco-index-add)
* [`myco index remove`↴](#myco-index-remove)
* [`myco index list`↴](#myco-index-list)
* [`myco index rebuild`↴](#myco-index-rebuild)
* [`myco org`↴](#myco-org)
* [`myco org init`↴](#myco-org-init)
* [`myco manifest`↴](#myco-manifest)
* [`myco schema`↴](#myco-schema)
* [`myco init`↴](#myco-init)
* [`myco check`↴](#myco-check)
* [`myco migrate`↴](#myco-migrate)
* [`myco commit`↴](#myco-commit)
* [`myco undo`↴](#myco-undo)
* [`myco redo`↴](#myco-redo)
* [`myco sync`↴](#myco-sync)
* [`myco view`↴](#myco-view)
* [`myco next`↴](#myco-next)
* [`myco status`↴](#myco-status)
* [`myco report`↴](#myco-report)
* [`myco handoff`↴](#myco-handoff)
* [`myco interrupt`↴](#myco-interrupt)
* [`myco reap`↴](#myco-reap)
* [`myco resume`↴](#myco-resume)
* [`myco ui`↴](#myco-ui)
* [`myco serve`↴](#myco-serve)
* [`myco batch`↴](#myco-batch)

## `myco`

myco — agent-first project management for a repo's `project/` worktree.

**Usage:** `myco [OPTIONS] <COMMAND>`

###### **Commands:**

* `todo` — TODO.md tasks — add / done / cancel / defer / list.
* `backlog` — BACKLOG.md parked items — add / promote / list.
* `done` — DONE.md — the archived-task archive (`todo done`/`todo cancel` land here; `item set --status open|deferred` reopens, `done remove` deletes).
* `issue` — Tracked issues, one file each under `issues/`.
* `proposal` — Design proposals under `proposals/` (`P-NNN`).
* `plan` — Execution plans under `plans/` (`PL-NNN`).
* `adr` — Architecture decision records under `adrs/` (`ADR-NNNN`).
* `item` — Kind-agnostic item verbs — show / set / move.
* `absorb` — Import work into the ledger — TODO/FIXME comments, Jira CSV exports, GitHub issue dumps.
* `index` — The derived index: register repos, query/rebuild snapshots.
* `org` — Cross-repo scaffolding — re-run `init` on every registered repo. Cross-repo *views* are `myco view org-board|org-status` (index-sourced).
* `manifest` — Capabilities + protocol version — agent self-discovery.
* `schema` — The file/state spec, machine-readable JSON Schema.
* `init` — Scaffold `project/`: orphan branch + worktree + .gitignore + seed files + the managed AGENTS.md agent-awareness block. Idempotent — existing worktree and AGENTS.md prose are preserved, only the marked block is (re)written.
* `check` — Validate the ledger (schemas, state machine, ids, refs) and the worktree it lives in (detached HEAD, wrong branch, unpushed commits); also validates the managed AGENTS.md block on the repo root. Findings carry a severity: `error` exits 1, `warning` is reported and costs nothing (ADR-0002). `--fix` applies safe repairs + commits, including reattaching a detached worktree.
* `migrate` — Convert legacy ledgers to v2: `pm/` → `project/`, or normalize files already on the `project` branch.
* `commit` — Stage + commit worktree changes (conventional message).
* `undo` — Undo the last ledger commit — resets the `project` branch tip one back (`HEAD~1`). One verb = one commit, so the reset is the inverse mutation, file deletions included; the undone commit is journaled for `redo`. Refuses on a dirty worktree (uncommitted `--no-commit` writes), at the root commit, and when the tip is already published on `origin` (unless `--force`). `--list` shows the redoable stack without mutating.
* `redo` — Redo the last undo — pops the undo journal and restores the commit `myco undo` moved away from (`git reset --hard <target>`). Repeatable while entries remain; the stack self-invalidates when the ledger moves (any commit/pull/manual git op since the undo). Refuses on a dirty worktree.
* `sync` — Reconcile the ledger branch with `origin/<branch>` — fetch, fast-forward or rebase local commits onto the remote tip, then heal merge damage: `meta.toml` counters reconcile, duplicate ids renumber remote-wins (the pushed item keeps its id; the local one moves to a fresh id, inbound `blocks:`/`links:`/supersede refs rebind to it, and a `renumbered` handoff entry records the audit). Mutating verbs refuse while behind (`ensure_synced`; `sync.auto_fetch` config fetches first), so this is the gate. A rebase conflict stops mid-flight — resolve + `git rebase --continue` + `myco sync` again, or `--abort` to bail. With `sync.auto_push` enabled (the default; `false` opts out), a successful sync also pushes the reconciled tip (best-effort — a push failure warns, never fails).
* `view` — Declarative views — named presets (`[views.<name>]` in config + built-ins `board|open|epic|milestone|next|roadmap|stats|burnup`, plus index-sourced `org-board|org-status` for cross-repo) or an ad-hoc spec from flags (`--source index` unions the index universe — lasius clones ∪ registered repos). `myco view board` is the kanban; `format = "repos"` emits one row per repo; `format = "timeline"|"stats"|"burnup"` are the analytics emits (roadmap lanes, count tallies, done-over-time — `done:` archive stamps drive burnup); bare `myco view` lists views (or renders `default_view` when config names one).
* `next` — The single highest-priority open item — "what should I work on" (a `view` eval with limit 1: open tasks+issues, `pri,id`; the filter flags merge over that default). `--claim` mutates it to `claimed` + an owner in one commit.
* `status` — Cheap digest of the work ledger. `--prompt` = token-sized.
* `report` — Aging + velocity + blocked — how the ledger is trending.
* `handoff` — Attach resumable state at session end — appends a dated entry to `handoffs/<ID>.md` (any item kind, live or archived; append-only).
* `interrupt` — A session ended with work still claimed/in-progress/blocked — revert the item to `open` + `interrupted` handoff, one commit. Without an id, `--session <id>` is the teardown: every item claimed under that session reverts in one commit. Session-lifecycle hooks (sepia, CLI session-end) call this; myco never detects sessions itself.
* `reap` — Bulk stale-claim reaper — every live `claimed`/`in-progress` item whose last activity (latest handoff entry, else `created`) is older than `--idle-days` reverts to `open` + an `interrupted` handoff, ONE commit for the whole sweep. `--session <id>` scopes to items whose claim tag names that session; `--dry-run` lists the sweep with zero writes. Scheduler-facing — peregrin campaigns call it periodically; the no-hook safety net behind `report`'s stale-claims finding.
* `resume` — Minimal context block to restart work on an item — the item, `blocks:` dep titles, latest handoffs, open checklist. The single-item `status --prompt`. 1..n ids batch in one call.
* `ui` — Interactive board TUI (ratatui) — TODO/BACKLOG tabs, item preview, writes through the same ops the verbs use; auto-reloads when the ledger changes on disk. `--view` with a `source = "index"` spec (org-*) opens the cross-repo index board: `repo:ID` rows, verbs write the owning repo.
* `serve` — Sequential request/response verb surface on stdio — JSON-RPC 2.0, one line in / one line out. For embedding (peregrin spawns one per repo); stdin IS the wire, so no served method reads content from it.
* `batch` — One-shot `serve` — NDJSON verb requests from stdin or `--file`, one JSON-RPC response line per request, dispatched through the same served surface. Per-line errors never fail the run; mutations keep per-request commits (`--no-commit`/`$MYCO_NO_COMMIT` apply). stdin IS the wire here too — requests that would read it are rejected.

###### **Options:**

* `--json` — Emit JSON (accepted everywhere; default output for machines).
* `--no-commit` — Skip the per-verb commit (also suppressed by $MYCO_NO_COMMIT).
* `--dir <PATH>` — Project worktree to act on, overriding discovery.
* `--root <PATH>` — Repo root to act on, overriding discovery (the worktree is `<root>/project`).
* `-h, --help` — Print help
* `-V, --version` — Print version

## `myco todo`

TODO.md tasks — add / done / cancel / defer / list.

**Usage:** `myco todo [OPTIONS] <COMMAND>`

###### **Commands:**

* `add` — Add a task — allocs a T-id, appends to a section of TODO.md.
* `done` — Mark done — walks the state machine to `done`, removes the line, and archives the subtree into DONE.md. Done on a parent archives the whole subtree — children die with it. Confirms on a tty (`--yes`/`$MYCO_YES` skips; scripts never prompt).
* `cancel` — Mark cancelled — walks the state machine to `cancelled` (archived like done but the work never happened), removes the line, and archives the subtree into DONE.md with `status:cancelled` preserved. Works on BACKLOG.md items too — no promote needed. Confirms on a tty (`--yes`/`$MYCO_YES` skips; scripts never prompt).
* `defer` — Park a task — transition to `deferred`, move the line to BACKLOG.md. Defer on a parent moves the whole subtree (children park with it); on a child it detaches alone.
* `list` — List tasks.

###### **Options:**

* `--json` — Emit JSON (accepted everywhere; default output for machines).
* `--no-commit` — Skip the per-verb commit (also suppressed by $MYCO_NO_COMMIT).
* `--dir <PATH>` — Project worktree to act on, overriding discovery.
* `--root <PATH>` — Repo root to act on, overriding discovery (the worktree is `<root>/project`).
* `-h, --help` — Print help

## `myco todo add`

Add a task — allocs a T-id, appends to a section of TODO.md.

**Usage:** `myco todo add [OPTIONS] <title>`

###### **Arguments:**

* `<title>` — One-line title.

###### **Options:**

* `--json` — Emit JSON (accepted everywhere; default output for machines).
* `--section <section>` — Section to add under (created when absent; default: the first section, or an unsectioned run).
* `--no-commit` — Skip the per-verb commit (also suppressed by $MYCO_NO_COMMIT).
* `--parent <parent>` — Nest under this task instead of a section — the new item becomes a child (` - [ ]` under the parent).
* `--dir <PATH>` — Project worktree to act on, overriding discovery.
* `--pri <pri>` — Priority: low|med|high|crit. [alias: --priority]
* `--owner <owner>` — Owner actor id (`devin`, `human:luis`, …).
* `--root <PATH>` — Repo root to act on, overriding discovery (the worktree is `<root>/project`).
* `--label <label>` — `#label` — repeatable.
* `--blocks <blocks>` — Ids this item blocks — comma-separated or repeatable.
* `--links <links>` — Ids this item is associated with (its plan, proposal, …) — comma-separated or repeatable.
* `--epic <epic>` — Epic name — the delivery grouping this item belongs to (`[A-Za-z0-9_.-]+`).
* `--milestone <milestone>` — Milestone name — the target this item ships under (`[A-Za-z0-9_.-]+`).
* `--due <due>` — Due date — `YYYY-MM-DD`.
* `--est <est>` — Estimate in points (0–1000) — relative size, not hours. [alias: --estimate]
* `--cycle <cycle>` — Cycle name — the iteration this item is scheduled in (`[A-Za-z0-9_.-]+`).
* `-h, --help` — Print help

## `myco todo done`

Mark done — walks the state machine to `done`, removes the line, and archives the subtree into DONE.md. Done on a parent archives the whole subtree — children die with it. Confirms on a tty (`--yes`/`$MYCO_YES` skips; scripts never prompt).

**Usage:** `myco todo done [OPTIONS] <id>`

###### **Arguments:**

* `<id>` — Task id (`T-NNN`).

###### **Options:**

* `--json` — Emit JSON (accepted everywhere; default output for machines).
* `--yes` — Skip the confirmation prompt (also `$MYCO_YES`; non-interactive runs never prompt).
* `--no-commit` — Skip the per-verb commit (also suppressed by $MYCO_NO_COMMIT).
* `--dir <PATH>` — Project worktree to act on, overriding discovery.
* `--root <PATH>` — Repo root to act on, overriding discovery (the worktree is `<root>/project`).
* `-h, --help` — Print help

## `myco todo cancel`

Mark cancelled — walks the state machine to `cancelled` (archived like done but the work never happened), removes the line, and archives the subtree into DONE.md with `status:cancelled` preserved. Works on BACKLOG.md items too — no promote needed. Confirms on a tty (`--yes`/`$MYCO_YES` skips; scripts never prompt).

**Usage:** `myco todo cancel [OPTIONS] <id>`

###### **Arguments:**

* `<id>` — Task id (`T-NNN`).

###### **Options:**

* `--json` — Emit JSON (accepted everywhere; default output for machines).
* `--yes` — Skip the confirmation prompt (also `$MYCO_YES`; non-interactive runs never prompt).
* `--no-commit` — Skip the per-verb commit (also suppressed by $MYCO_NO_COMMIT).
* `--dir <PATH>` — Project worktree to act on, overriding discovery.
* `--root <PATH>` — Repo root to act on, overriding discovery (the worktree is `<root>/project`).
* `-h, --help` — Print help

## `myco todo defer`

Park a task — transition to `deferred`, move the line to BACKLOG.md. Defer on a parent moves the whole subtree (children park with it); on a child it detaches alone.

**Usage:** `myco todo defer [OPTIONS] <id>`

###### **Arguments:**

* `<id>` — Task id (`T-NNN`).

###### **Options:**

* `--json` — Emit JSON (accepted everywhere; default output for machines).
* `--no-commit` — Skip the per-verb commit (also suppressed by $MYCO_NO_COMMIT).
* `--dir <PATH>` — Project worktree to act on, overriding discovery.
* `--root <PATH>` — Repo root to act on, overriding discovery (the worktree is `<root>/project`).
* `-h, --help` — Print help

## `myco todo list`

List tasks.

**Usage:** `myco todo list [OPTIONS]`

###### **Options:**

* `--json` — Emit JSON (accepted everywhere; default output for machines).
* `--status <status>` — Keep only items with this status.
* `--no-commit` — Skip the per-verb commit (also suppressed by $MYCO_NO_COMMIT).
* `--owner <owner>` — Keep only items owned by this actor.
* `--dir <PATH>` — Project worktree to act on, overriding discovery.
* `--label <label>` — Keep only items carrying all of these labels (repeatable).
* `--pri <pri>` — Keep only items at this priority.
* `--root <PATH>` — Repo root to act on, overriding discovery (the worktree is `<root>/project`).
* `--epic <epic>` — Keep only items in this epic.
* `--milestone <milestone>` — Keep only items in this milestone.
* `--due <due>` — Keep only items due on this date (`YYYY-MM-DD`).
* `--est <est>` — Keep only items with this point estimate. [alias: --estimate]
* `--cycle <cycle>` — Keep only items in this cycle.
* `--query <query>` — Keep only items whose id, title, or a label contains this text (case-insensitive).
* `--limit <N>` — Keep only the first N items after filtering.
* `-h, --help` — Print help

## `myco backlog`

BACKLOG.md parked items — add / promote / list.

**Usage:** `myco backlog [OPTIONS] <COMMAND>`

###### **Commands:**

* `add` — Add a deferred item — allocs a T-id straight into BACKLOG.md.
* `promote` — Re-activate a parked item — `deferred → open`, move the line to TODO.md.
* `list` — List backlog items.

###### **Options:**

* `--json` — Emit JSON (accepted everywhere; default output for machines).
* `--no-commit` — Skip the per-verb commit (also suppressed by $MYCO_NO_COMMIT).
* `--dir <PATH>` — Project worktree to act on, overriding discovery.
* `--root <PATH>` — Repo root to act on, overriding discovery (the worktree is `<root>/project`).
* `-h, --help` — Print help

## `myco backlog add`

Add a deferred item — allocs a T-id straight into BACKLOG.md.

**Usage:** `myco backlog add [OPTIONS] <title>`

###### **Arguments:**

* `<title>` — One-line title.

###### **Options:**

* `--json` — Emit JSON (accepted everywhere; default output for machines).
* `--section <section>` — Section to add under (created when absent; default: the first section, or an unsectioned run).
* `--no-commit` — Skip the per-verb commit (also suppressed by $MYCO_NO_COMMIT).
* `--pri <pri>` — Priority: low|med|high|crit. [alias: --priority]
* `--dir <PATH>` — Project worktree to act on, overriding discovery.
* `--owner <owner>` — Owner actor id (`devin`, `human:luis`, …).
* `--label <label>` — `#label` — repeatable.
* `--root <PATH>` — Repo root to act on, overriding discovery (the worktree is `<root>/project`).
* `--blocks <blocks>` — Ids this item blocks — comma-separated or repeatable.
* `--links <links>` — Ids this item is associated with (its plan, proposal, …) — comma-separated or repeatable.
* `--epic <epic>` — Epic name — the delivery grouping this item belongs to (`[A-Za-z0-9_.-]+`).
* `--milestone <milestone>` — Milestone name — the target this item ships under (`[A-Za-z0-9_.-]+`).
* `--due <due>` — Due date — `YYYY-MM-DD`.
* `--est <est>` — Estimate in points (0–1000) — relative size, not hours. [alias: --estimate]
* `--cycle <cycle>` — Cycle name — the iteration this item is scheduled in (`[A-Za-z0-9_.-]+`).
* `-h, --help` — Print help

## `myco backlog promote`

Re-activate a parked item — `deferred → open`, move the line to TODO.md.

**Usage:** `myco backlog promote [OPTIONS] <id>`

###### **Arguments:**

* `<id>` — Task id (`T-NNN`).

###### **Options:**

* `--json` — Emit JSON (accepted everywhere; default output for machines).
* `--no-commit` — Skip the per-verb commit (also suppressed by $MYCO_NO_COMMIT).
* `--dir <PATH>` — Project worktree to act on, overriding discovery.
* `--root <PATH>` — Repo root to act on, overriding discovery (the worktree is `<root>/project`).
* `-h, --help` — Print help

## `myco backlog list`

List backlog items.

**Usage:** `myco backlog list [OPTIONS]`

###### **Options:**

* `--json` — Emit JSON (accepted everywhere; default output for machines).
* `--status <status>` — Keep only items with this status.
* `--no-commit` — Skip the per-verb commit (also suppressed by $MYCO_NO_COMMIT).
* `--owner <owner>` — Keep only items owned by this actor.
* `--dir <PATH>` — Project worktree to act on, overriding discovery.
* `--label <label>` — Keep only items carrying all of these labels (repeatable).
* `--pri <pri>` — Keep only items at this priority.
* `--root <PATH>` — Repo root to act on, overriding discovery (the worktree is `<root>/project`).
* `--epic <epic>` — Keep only items in this epic.
* `--milestone <milestone>` — Keep only items in this milestone.
* `--due <due>` — Keep only items due on this date (`YYYY-MM-DD`).
* `--est <est>` — Keep only items with this point estimate. [alias: --estimate]
* `--cycle <cycle>` — Keep only items in this cycle.
* `--query <query>` — Keep only items whose id, title, or a label contains this text (case-insensitive).
* `--limit <N>` — Keep only the first N items after filtering.
* `-h, --help` — Print help

## `myco done`

DONE.md — the archived-task archive (`todo done`/`todo cancel` land here; `item set --status open|deferred` reopens, `done remove` deletes).

**Usage:** `myco done [OPTIONS] <COMMAND>`

###### **Commands:**

* `list` — List archived tasks — the DONE.md archive `todo done`/`todo cancel`/`item set --status done|cancelled` land in. Read-only; `item set --status open|deferred` reopens one back into the live docs.
* `remove` — Delete an archived task subtree from DONE.md outright — the ledger forgets it (scaffold junk, mistakes). Reopen unfinished work with `item set --status open` instead. Confirms on a tty (`--yes`/`$MYCO_YES` skips; scripts never prompt).

###### **Options:**

* `--json` — Emit JSON (accepted everywhere; default output for machines).
* `--no-commit` — Skip the per-verb commit (also suppressed by $MYCO_NO_COMMIT).
* `--dir <PATH>` — Project worktree to act on, overriding discovery.
* `--root <PATH>` — Repo root to act on, overriding discovery (the worktree is `<root>/project`).
* `-h, --help` — Print help

## `myco done list`

List archived tasks — the DONE.md archive `todo done`/`todo cancel`/`item set --status done|cancelled` land in. Read-only; `item set --status open|deferred` reopens one back into the live docs.

**Usage:** `myco done list [OPTIONS]`

###### **Options:**

* `--json` — Emit JSON (accepted everywhere; default output for machines).
* `--status <status>` — Keep only items with this status.
* `--no-commit` — Skip the per-verb commit (also suppressed by $MYCO_NO_COMMIT).
* `--owner <owner>` — Keep only items owned by this actor.
* `--dir <PATH>` — Project worktree to act on, overriding discovery.
* `--label <label>` — Keep only items carrying all of these labels (repeatable).
* `--pri <pri>` — Keep only items at this priority.
* `--root <PATH>` — Repo root to act on, overriding discovery (the worktree is `<root>/project`).
* `--epic <epic>` — Keep only items in this epic.
* `--milestone <milestone>` — Keep only items in this milestone.
* `--due <due>` — Keep only items due on this date (`YYYY-MM-DD`).
* `--est <est>` — Keep only items with this point estimate. [alias: --estimate]
* `--cycle <cycle>` — Keep only items in this cycle.
* `--query <query>` — Keep only items whose id, title, or a label contains this text (case-insensitive).
* `--limit <N>` — Keep only the first N items after filtering.
* `-h, --help` — Print help

## `myco done remove`

Delete an archived task subtree from DONE.md outright — the ledger forgets it (scaffold junk, mistakes). Reopen unfinished work with `item set --status open` instead. Confirms on a tty (`--yes`/`$MYCO_YES` skips; scripts never prompt).

**Usage:** `myco done remove [OPTIONS] <id>`

###### **Arguments:**

* `<id>` — Task id (`T-NNN`).

###### **Options:**

* `--json` — Emit JSON (accepted everywhere; default output for machines).
* `--yes` — Skip the confirmation prompt (also `$MYCO_YES`; non-interactive runs never prompt).
* `--no-commit` — Skip the per-verb commit (also suppressed by $MYCO_NO_COMMIT).
* `--dir <PATH>` — Project worktree to act on, overriding discovery.
* `--root <PATH>` — Repo root to act on, overriding discovery (the worktree is `<root>/project`).
* `-h, --help` — Print help

## `myco issue`

Tracked issues, one file each under `issues/`.

**Usage:** `myco issue [OPTIONS] <COMMAND>`

###### **Commands:**

* `new` — File an issue — allocs an I-id, writes `issues/issue-NNNN-<slug>.md` with frontmatter + template.
* `list` — List issues (live files; `--archived` includes `issues/archive/`).
* `resolve` — Close an issue — walks the state machine to `done`; `--resolution` appends text under `## Resolution`.
* `archive` — Shelve a done issue — moves the file to `issues/archive/`.
* `comment` — Append a dated note under `## Comments` (created when missing).

###### **Options:**

* `--json` — Emit JSON (accepted everywhere; default output for machines).
* `--no-commit` — Skip the per-verb commit (also suppressed by $MYCO_NO_COMMIT).
* `--dir <PATH>` — Project worktree to act on, overriding discovery.
* `--root <PATH>` — Repo root to act on, overriding discovery (the worktree is `<root>/project`).
* `-h, --help` — Print help

## `myco issue new`

File an issue — allocs an I-id, writes `issues/issue-NNNN-<slug>.md` with frontmatter + template.

**Usage:** `myco issue new [OPTIONS] <title>`

###### **Arguments:**

* `<title>` — One-line title (drives the filename slug).

###### **Options:**

* `--body <FILE|->` — Body markdown from a file, or `-` for stdin.
* `--json` — Emit JSON (accepted everywhere; default output for machines).
* `--no-commit` — Skip the per-verb commit (also suppressed by $MYCO_NO_COMMIT).
* `--text <text>` — Body markdown inline.
* `--dir <PATH>` — Project worktree to act on, overriding discovery.
* `--pri <pri>` — Priority: low|med|high|crit. [alias: --priority]
* `--owner <owner>` — Owner actor id (`devin`, `human:luis`, …).
* `--root <PATH>` — Repo root to act on, overriding discovery (the worktree is `<root>/project`).
* `--label <label>` — `#label` — repeatable.
* `--blocks <blocks>` — Ids this item blocks — comma-separated or repeatable.
* `--links <links>` — Ids this item is associated with (its plan, proposal, …) — comma-separated or repeatable.
* `--epic <epic>` — Epic name — the delivery grouping this item belongs to (`[A-Za-z0-9_.-]+`).
* `--milestone <milestone>` — Milestone name — the target this item ships under (`[A-Za-z0-9_.-]+`).
* `--due <due>` — Due date — `YYYY-MM-DD`.
* `--est <est>` — Estimate in points (0–1000) — relative size, not hours. [alias: --estimate]
* `--cycle <cycle>` — Cycle name — the iteration this item is scheduled in (`[A-Za-z0-9_.-]+`).
* `-h, --help` — Print help

## `myco issue list`

List issues (live files; `--archived` includes `issues/archive/`).

**Usage:** `myco issue list [OPTIONS]`

###### **Options:**

* `--archived` — Include archived items.
* `--json` — Emit JSON (accepted everywhere; default output for machines).
* `--no-commit` — Skip the per-verb commit (also suppressed by $MYCO_NO_COMMIT).
* `--status <status>` — Keep only items with this status.
* `--dir <PATH>` — Project worktree to act on, overriding discovery.
* `--owner <owner>` — Keep only items owned by this actor.
* `--label <label>` — Keep only items carrying all of these labels (repeatable).
* `--root <PATH>` — Repo root to act on, overriding discovery (the worktree is `<root>/project`).
* `--pri <pri>` — Keep only items at this priority.
* `--epic <epic>` — Keep only items in this epic.
* `--milestone <milestone>` — Keep only items in this milestone.
* `--due <due>` — Keep only items due on this date (`YYYY-MM-DD`).
* `--est <est>` — Keep only items with this point estimate. [alias: --estimate]
* `--cycle <cycle>` — Keep only items in this cycle.
* `--query <query>` — Keep only items whose id, title, or a label contains this text (case-insensitive).
* `--limit <N>` — Keep only the first N items after filtering.
* `-h, --help` — Print help

## `myco issue resolve`

Close an issue — walks the state machine to `done`; `--resolution` appends text under `## Resolution`.

**Usage:** `myco issue resolve [OPTIONS] <id>`

###### **Arguments:**

* `<id>` — Issue id (`I-NNN`).

###### **Options:**

* `--json` — Emit JSON (accepted everywhere; default output for machines).
* `--resolution <resolution>` — Resolution text appended to the body.
* `--no-commit` — Skip the per-verb commit (also suppressed by $MYCO_NO_COMMIT).
* `--dir <PATH>` — Project worktree to act on, overriding discovery.
* `--root <PATH>` — Repo root to act on, overriding discovery (the worktree is `<root>/project`).
* `-h, --help` — Print help

## `myco issue archive`

Shelve a done issue — moves the file to `issues/archive/`.

**Usage:** `myco issue archive [OPTIONS] <id>`

###### **Arguments:**

* `<id>` — Issue id (`I-NNN`).

###### **Options:**

* `--json` — Emit JSON (accepted everywhere; default output for machines).
* `--no-commit` — Skip the per-verb commit (also suppressed by $MYCO_NO_COMMIT).
* `--dir <PATH>` — Project worktree to act on, overriding discovery.
* `--root <PATH>` — Repo root to act on, overriding discovery (the worktree is `<root>/project`).
* `-h, --help` — Print help

## `myco issue comment`

Append a dated note under `## Comments` (created when missing).

**Usage:** `myco issue comment [OPTIONS] <id> <text>`

###### **Arguments:**

* `<id>` — Issue id (`I-NNN`).
* `<text>` — Comment text.

###### **Options:**

* `--json` — Emit JSON (accepted everywhere; default output for machines).
* `--no-commit` — Skip the per-verb commit (also suppressed by $MYCO_NO_COMMIT).
* `--dir <PATH>` — Project worktree to act on, overriding discovery.
* `--root <PATH>` — Repo root to act on, overriding discovery (the worktree is `<root>/project`).
* `-h, --help` — Print help

## `myco proposal`

Design proposals under `proposals/` (`P-NNN`).

**Usage:** `myco proposal [OPTIONS] <COMMAND>`

###### **Commands:**

* `new` — File a proposal — allocs a P-id, writes the proposal file with frontmatter + template.
* `list` — List proposals.
* `status` — Set a proposal's status. Proposal words alias the state machine: `draft` = open, `accepted` = done, `rejected` = deferred — the transition is walked through the machine, so `accepted` passes `open → in-progress → done`.

###### **Options:**

* `--json` — Emit JSON (accepted everywhere; default output for machines).
* `--no-commit` — Skip the per-verb commit (also suppressed by $MYCO_NO_COMMIT).
* `--dir <PATH>` — Project worktree to act on, overriding discovery.
* `--root <PATH>` — Repo root to act on, overriding discovery (the worktree is `<root>/project`).
* `-h, --help` — Print help

## `myco proposal new`

File a proposal — allocs a P-id, writes the proposal file with frontmatter + template.

**Usage:** `myco proposal new [OPTIONS] <title>`

###### **Arguments:**

* `<title>` — One-line title (drives the filename slug).

###### **Options:**

* `--body <FILE|->` — Body markdown from a file, or `-` for stdin.
* `--json` — Emit JSON (accepted everywhere; default output for machines).
* `--no-commit` — Skip the per-verb commit (also suppressed by $MYCO_NO_COMMIT).
* `--text <text>` — Body markdown inline.
* `--dir <PATH>` — Project worktree to act on, overriding discovery.
* `--pri <pri>` — Priority: low|med|high|crit. [alias: --priority]
* `--owner <owner>` — Owner actor id (`devin`, `human:luis`, …).
* `--root <PATH>` — Repo root to act on, overriding discovery (the worktree is `<root>/project`).
* `--label <label>` — `#label` — repeatable.
* `--blocks <blocks>` — Ids this item blocks — comma-separated or repeatable.
* `--links <links>` — Ids this item is associated with (its plan, proposal, …) — comma-separated or repeatable.
* `--epic <epic>` — Epic name — the delivery grouping this item belongs to (`[A-Za-z0-9_.-]+`).
* `--milestone <milestone>` — Milestone name — the target this item ships under (`[A-Za-z0-9_.-]+`).
* `--due <due>` — Due date — `YYYY-MM-DD`.
* `--est <est>` — Estimate in points (0–1000) — relative size, not hours. [alias: --estimate]
* `--cycle <cycle>` — Cycle name — the iteration this item is scheduled in (`[A-Za-z0-9_.-]+`).
* `-h, --help` — Print help

## `myco proposal list`

List proposals.

**Usage:** `myco proposal list [OPTIONS]`

###### **Options:**

* `--json` — Emit JSON (accepted everywhere; default output for machines).
* `--status <status>` — Keep only items with this status.
* `--no-commit` — Skip the per-verb commit (also suppressed by $MYCO_NO_COMMIT).
* `--owner <owner>` — Keep only items owned by this actor.
* `--dir <PATH>` — Project worktree to act on, overriding discovery.
* `--label <label>` — Keep only items carrying all of these labels (repeatable).
* `--pri <pri>` — Keep only items at this priority.
* `--root <PATH>` — Repo root to act on, overriding discovery (the worktree is `<root>/project`).
* `--epic <epic>` — Keep only items in this epic.
* `--milestone <milestone>` — Keep only items in this milestone.
* `--due <due>` — Keep only items due on this date (`YYYY-MM-DD`).
* `--est <est>` — Keep only items with this point estimate. [alias: --estimate]
* `--cycle <cycle>` — Keep only items in this cycle.
* `--query <query>` — Keep only items whose id, title, or a label contains this text (case-insensitive).
* `--limit <N>` — Keep only the first N items after filtering.
* `-h, --help` — Print help

## `myco proposal status`

Set a proposal's status. Proposal words alias the state machine: `draft` = open, `accepted` = done, `rejected` = deferred — the transition is walked through the machine, so `accepted` passes `open → in-progress → done`.

**Usage:** `myco proposal status [OPTIONS] <id> <status>`

###### **Arguments:**

* `<id>` — Proposal id (`P-NNN`).
* `<status>` — `draft` | `accepted` | `rejected` (raw machine statuses like `in-progress` also accepted).

###### **Options:**

* `--json` — Emit JSON (accepted everywhere; default output for machines).
* `--no-commit` — Skip the per-verb commit (also suppressed by $MYCO_NO_COMMIT).
* `--dir <PATH>` — Project worktree to act on, overriding discovery.
* `--root <PATH>` — Repo root to act on, overriding discovery (the worktree is `<root>/project`).
* `-h, --help` — Print help

## `myco plan`

Execution plans under `plans/` (`PL-NNN`).

**Usage:** `myco plan [OPTIONS] <COMMAND>`

###### **Commands:**

* `new` — File a plan — allocs a PL-id, writes the plan file with frontmatter + a phase-checklist template.
* `list` — List plans.

###### **Options:**

* `--json` — Emit JSON (accepted everywhere; default output for machines).
* `--no-commit` — Skip the per-verb commit (also suppressed by $MYCO_NO_COMMIT).
* `--dir <PATH>` — Project worktree to act on, overriding discovery.
* `--root <PATH>` — Repo root to act on, overriding discovery (the worktree is `<root>/project`).
* `-h, --help` — Print help

## `myco plan new`

File a plan — allocs a PL-id, writes the plan file with frontmatter + a phase-checklist template.

**Usage:** `myco plan new [OPTIONS] <title>`

###### **Arguments:**

* `<title>` — One-line title (drives the filename slug).

###### **Options:**

* `--body <FILE|->` — Body markdown from a file, or `-` for stdin.
* `--json` — Emit JSON (accepted everywhere; default output for machines).
* `--no-commit` — Skip the per-verb commit (also suppressed by $MYCO_NO_COMMIT).
* `--text <text>` — Body markdown inline.
* `--dir <PATH>` — Project worktree to act on, overriding discovery.
* `--pri <pri>` — Priority: low|med|high|crit. [alias: --priority]
* `--owner <owner>` — Owner actor id (`devin`, `human:luis`, …).
* `--root <PATH>` — Repo root to act on, overriding discovery (the worktree is `<root>/project`).
* `--label <label>` — `#label` — repeatable.
* `--blocks <blocks>` — Ids this item blocks — comma-separated or repeatable.
* `--links <links>` — Ids this item is associated with (its plan, proposal, …) — comma-separated or repeatable.
* `--epic <epic>` — Epic name — the delivery grouping this item belongs to (`[A-Za-z0-9_.-]+`).
* `--milestone <milestone>` — Milestone name — the target this item ships under (`[A-Za-z0-9_.-]+`).
* `--due <due>` — Due date — `YYYY-MM-DD`.
* `--est <est>` — Estimate in points (0–1000) — relative size, not hours. [alias: --estimate]
* `--cycle <cycle>` — Cycle name — the iteration this item is scheduled in (`[A-Za-z0-9_.-]+`).
* `-h, --help` — Print help

## `myco plan list`

List plans.

**Usage:** `myco plan list [OPTIONS]`

###### **Options:**

* `--json` — Emit JSON (accepted everywhere; default output for machines).
* `--status <status>` — Keep only items with this status.
* `--no-commit` — Skip the per-verb commit (also suppressed by $MYCO_NO_COMMIT).
* `--owner <owner>` — Keep only items owned by this actor.
* `--dir <PATH>` — Project worktree to act on, overriding discovery.
* `--label <label>` — Keep only items carrying all of these labels (repeatable).
* `--pri <pri>` — Keep only items at this priority.
* `--root <PATH>` — Repo root to act on, overriding discovery (the worktree is `<root>/project`).
* `--epic <epic>` — Keep only items in this epic.
* `--milestone <milestone>` — Keep only items in this milestone.
* `--due <due>` — Keep only items due on this date (`YYYY-MM-DD`).
* `--est <est>` — Keep only items with this point estimate. [alias: --estimate]
* `--cycle <cycle>` — Keep only items in this cycle.
* `--query <query>` — Keep only items whose id, title, or a label contains this text (case-insensitive).
* `--limit <N>` — Keep only the first N items after filtering.
* `-h, --help` — Print help

## `myco adr`

Architecture decision records under `adrs/` (`ADR-NNNN`).

**Usage:** `myco adr [OPTIONS] <COMMAND>`

###### **Commands:**

* `new` — File an ADR — allocs an ADR-id, writes the file with frontmatter (status `open` = "proposed") + template.
* `list` — List ADRs.
* `supersede` — Mark an ADR superseded by another — the old file gets `superseded_by:` + status `done`, the replacement gets a reciprocal `supersedes:` key. Both must already exist.

###### **Options:**

* `--json` — Emit JSON (accepted everywhere; default output for machines).
* `--no-commit` — Skip the per-verb commit (also suppressed by $MYCO_NO_COMMIT).
* `--dir <PATH>` — Project worktree to act on, overriding discovery.
* `--root <PATH>` — Repo root to act on, overriding discovery (the worktree is `<root>/project`).
* `-h, --help` — Print help

## `myco adr new`

File an ADR — allocs an ADR-id, writes the file with frontmatter (status `open` = "proposed") + template.

**Usage:** `myco adr new [OPTIONS] <title>`

###### **Arguments:**

* `<title>` — One-line title (drives the filename slug).

###### **Options:**

* `--body <FILE|->` — Body markdown from a file, or `-` for stdin.
* `--json` — Emit JSON (accepted everywhere; default output for machines).
* `--no-commit` — Skip the per-verb commit (also suppressed by $MYCO_NO_COMMIT).
* `--text <text>` — Body markdown inline.
* `--dir <PATH>` — Project worktree to act on, overriding discovery.
* `--pri <pri>` — Priority: low|med|high|crit. [alias: --priority]
* `--owner <owner>` — Owner actor id (`devin`, `human:luis`, …).
* `--root <PATH>` — Repo root to act on, overriding discovery (the worktree is `<root>/project`).
* `--label <label>` — `#label` — repeatable.
* `--blocks <blocks>` — Ids this item blocks — comma-separated or repeatable.
* `--links <links>` — Ids this item is associated with (its plan, proposal, …) — comma-separated or repeatable.
* `--epic <epic>` — Epic name — the delivery grouping this item belongs to (`[A-Za-z0-9_.-]+`).
* `--milestone <milestone>` — Milestone name — the target this item ships under (`[A-Za-z0-9_.-]+`).
* `--due <due>` — Due date — `YYYY-MM-DD`.
* `--est <est>` — Estimate in points (0–1000) — relative size, not hours. [alias: --estimate]
* `--cycle <cycle>` — Cycle name — the iteration this item is scheduled in (`[A-Za-z0-9_.-]+`).
* `-h, --help` — Print help

## `myco adr list`

List ADRs.

**Usage:** `myco adr list [OPTIONS]`

###### **Options:**

* `--json` — Emit JSON (accepted everywhere; default output for machines).
* `--status <status>` — Keep only items with this status.
* `--no-commit` — Skip the per-verb commit (also suppressed by $MYCO_NO_COMMIT).
* `--owner <owner>` — Keep only items owned by this actor.
* `--dir <PATH>` — Project worktree to act on, overriding discovery.
* `--label <label>` — Keep only items carrying all of these labels (repeatable).
* `--pri <pri>` — Keep only items at this priority.
* `--root <PATH>` — Repo root to act on, overriding discovery (the worktree is `<root>/project`).
* `--epic <epic>` — Keep only items in this epic.
* `--milestone <milestone>` — Keep only items in this milestone.
* `--due <due>` — Keep only items due on this date (`YYYY-MM-DD`).
* `--est <est>` — Keep only items with this point estimate. [alias: --estimate]
* `--cycle <cycle>` — Keep only items in this cycle.
* `--query <query>` — Keep only items whose id, title, or a label contains this text (case-insensitive).
* `--limit <N>` — Keep only the first N items after filtering.
* `-h, --help` — Print help

## `myco adr supersede`

Mark an ADR superseded by another — the old file gets `superseded_by:` + status `done`, the replacement gets a reciprocal `supersedes:` key. Both must already exist.

**Usage:** `myco adr supersede [OPTIONS] --by <by> <id>`

###### **Arguments:**

* `<id>` — The ADR being replaced (`ADR-NNNN`).

###### **Options:**

* `--by <by>` — The ADR replacing it (`ADR-NNNN`).
* `--json` — Emit JSON (accepted everywhere; default output for machines).
* `--no-commit` — Skip the per-verb commit (also suppressed by $MYCO_NO_COMMIT).
* `--dir <PATH>` — Project worktree to act on, overriding discovery.
* `--root <PATH>` — Repo root to act on, overriding discovery (the worktree is `<root>/project`).
* `-h, --help` — Print help

## `myco item`

Kind-agnostic item verbs — show / set / move.

**Usage:** `myco item [OPTIONS] <COMMAND>`

###### **Commands:**

* `show` — Dump items — frontmatter fields, kind-grouped related edges (blocks/blocked_by/supersedes/associated/references/handoff, live-derived), plus the body for file-backed kinds (`--json` = the full Item + location + related + body). 1..n ids: a batch repeats the block in argument order; `--json` emits the per-id `[{id, ok, …}]` array.
* `set` — Mutate an item in place. `--status` is the only field routed through the state machine — single steps only, invalid ones error with the valid targets. `--title`/`--note` rewrite the headline fields (a file item's slug — and so its filename — follows the title). Status changes on tasks move the line between TODO.md and BACKLOG.md (`done`/`cancelled` archive the subtree into DONE.md — both confirm on a tty, and both reopen via `--status open|deferred`, also confirmed: `--yes`/`$MYCO_YES` skips).
* `move` — Move an item to another repo's ledger — re-creates it in the destination's own id sequence (kind, title, body, pri/owner/labels/epic/milestone carried; `blocks:`/`links:` dropped — they name source-ledger ids — and the drop is recorded as plain text in both provenance handoffs), writes a `moved from`/`moved to` provenance handoff on both sides, then walks the source to `done` (a task's line leaves the ledger; its handoff drops to `handoffs/archive/`). `--to` takes the destination repo root, its `project/` worktree, or an index-universe repo name (`myco index list`). Two commits — one per worktree. `--dry-run` previews the plan (prospective dest id, dropped refs) with no writes; `--finish` completes a move that died after the destination copy + `moved to` pointer landed but before the source close committed — the pointer is the resume handle.
* `reorder` — Sibling reorder inside TODO.md/BACKLOG.md — the TUI's `Alt+↑/↓` as a verb: `--delta -1` swaps with the previous sibling, `+1` with the next (larger deltas walk that many steps, stopping at the list edge). `Ledger::move_within` semantics exactly — a move never crosses a `##` section boundary, and a nested child swaps among its own siblings only. One commit per call (`chore: reorder <id>`); tasks only — file-backed items have no siblings.

###### **Options:**

* `--json` — Emit JSON (accepted everywhere; default output for machines).
* `--no-commit` — Skip the per-verb commit (also suppressed by $MYCO_NO_COMMIT).
* `--dir <PATH>` — Project worktree to act on, overriding discovery.
* `--root <PATH>` — Repo root to act on, overriding discovery (the worktree is `<root>/project`).
* `-h, --help` — Print help

## `myco item show`

Dump items — frontmatter fields, kind-grouped related edges (blocks/blocked_by/supersedes/associated/references/handoff, live-derived), plus the body for file-backed kinds (`--json` = the full Item + location + related + body). 1..n ids: a batch repeats the block in argument order; `--json` emits the per-id `[{id, ok, …}]` array.

**Usage:** `myco item show [OPTIONS] <id>...`

###### **Arguments:**

* `<id>...` — Any item id (`T-NNN`, `I-NNN`, `P-NNN`, `ADR-NNNN`, `PL-NNN`) — live or archived.

###### **Options:**

* `--json` — Emit JSON (accepted everywhere; default output for machines).
* `--no-commit` — Skip the per-verb commit (also suppressed by $MYCO_NO_COMMIT).
* `--dir <PATH>` — Project worktree to act on, overriding discovery.
* `--root <PATH>` — Repo root to act on, overriding discovery (the worktree is `<root>/project`).
* `-h, --help` — Print help

## `myco item set`

Mutate an item in place. `--status` is the only field routed through the state machine — single steps only, invalid ones error with the valid targets. `--title`/`--note` rewrite the headline fields (a file item's slug — and so its filename — follows the title). Status changes on tasks move the line between TODO.md and BACKLOG.md (`done`/`cancelled` archive the subtree into DONE.md — both confirm on a tty, and both reopen via `--status open|deferred`, also confirmed: `--yes`/`$MYCO_YES` skips).

**Usage:** `myco item set [OPTIONS] <id>`

###### **Arguments:**

* `<id>` — Any item id.

###### **Options:**

* `--json` — Emit JSON (accepted everywhere; default output for machines).
* `--title <title>` — New title — one line, no `·`; `""` clears it (tasks emit a bare id, file items an `untitled` slug and a renamed file).
* `--no-commit` — Skip the per-verb commit (also suppressed by $MYCO_NO_COMMIT).
* `--status <status>` — New status — one legal step from the current one.
* `--dir <PATH>` — Project worktree to act on, overriding discovery.
* `--pri <pri>` — Priority: low|med|high|crit. [alias: --priority]
* `--owner <owner>` — Owner actor id.
* `--root <PATH>` — Repo root to act on, overriding discovery (the worktree is `<root>/project`).
* `--label <label>` — Add (`+x` or `x`) or remove (`-x`) a label — repeatable.
* `--unlabel <unlabel>` — Remove a label — repeatable.
* `--blocks <blocks>` — Add (`+T-1` or `T-1`) or remove (`-T-1`) a blocks dep — repeatable or comma-separated.
* `--unblock <unblock>` — Remove a blocks dep — repeatable or comma-separated.
* `--links <links>` — Add (`+PL-1` or `PL-1`) or remove (`-PL-1`) a link — repeatable or comma-separated.
* `--unlink <unlink>` — Remove a link — repeatable or comma-separated.
* `--epic <epic>` — Epic name — `""` clears the epic.
* `--milestone <milestone>` — Milestone name — `""` clears the milestone.
* `--due <due>` — Due date (`YYYY-MM-DD`) — `""` clears the due date.
* `--est <est>` — Estimate in points (0–1000) — `""` clears the estimate. [alias: --estimate]
* `--cycle <cycle>` — Cycle name (`[A-Za-z0-9_.-]+`) — `""` clears the cycle.
* `--note <note>` — Free-form note (`note:` token / frontmatter `note`) — one line, no `·`; `""` clears it.
* `--session <session>` — Tag the item's claim with a session id — appends a `claimed` handoff entry carrying it (`interrupt --session` scopes teardown to it). The item must land in a work state.
* `--steal` — Take the item even when another actor owns the claim — records a `taken from` handoff entry.
* `--yes` — Skip the archive/reopen confirmation prompts (also `$MYCO_YES`; non-interactive runs never prompt).
* `-h, --help` — Print help

## `myco item move`

Move an item to another repo's ledger — re-creates it in the destination's own id sequence (kind, title, body, pri/owner/labels/epic/milestone carried; `blocks:`/`links:` dropped — they name source-ledger ids — and the drop is recorded as plain text in both provenance handoffs), writes a `moved from`/`moved to` provenance handoff on both sides, then walks the source to `done` (a task's line leaves the ledger; its handoff drops to `handoffs/archive/`). `--to` takes the destination repo root, its `project/` worktree, or an index-universe repo name (`myco index list`). Two commits — one per worktree. `--dry-run` previews the plan (prospective dest id, dropped refs) with no writes; `--finish` completes a move that died after the destination copy + `moved to` pointer landed but before the source close committed — the pointer is the resume handle.

**Usage:** `myco item move [OPTIONS] <id>`

###### **Arguments:**

* `<id>` — Any item id.

###### **Options:**

* `--json` — Emit JSON (accepted everywhere; default output for machines).
* `--to <to>` — Destination — a repo root, its `project/` worktree, or a registered repo name. Required unless `--finish`.
* `--no-commit` — Skip the per-verb commit (also suppressed by $MYCO_NO_COMMIT).
* `--note <note>` — Extra context appended to the source's `moved to` handoff note.
* `--actor <actor>` — Who moved it (default: resolved actor).
* `--dir <PATH>` — Project worktree to act on, overriding discovery.
* `--dry-run` — Validate + report the plan (prospective destination id, dropped refs) — no writes, no commits.
* `--root <PATH>` — Repo root to act on, overriding discovery (the worktree is `<root>/project`).
* `--finish` — Complete a partial move — finds the `moved to` handoff pointer, verifies the destination copy exists, closes the source, commits both worktrees. No `--to`.
* `--yes` — Skip the confirmation prompt (also `$MYCO_YES`; non-interactive runs never prompt).
* `-h, --help` — Print help

## `myco item reorder`

Sibling reorder inside TODO.md/BACKLOG.md — the TUI's `Alt+↑/↓` as a verb: `--delta -1` swaps with the previous sibling, `+1` with the next (larger deltas walk that many steps, stopping at the list edge). `Ledger::move_within` semantics exactly — a move never crosses a `##` section boundary, and a nested child swaps among its own siblings only. One commit per call (`chore: reorder <id>`); tasks only — file-backed items have no siblings.

**Usage:** `myco item reorder [OPTIONS] --delta <N> <id>`

###### **Arguments:**

* `<id>` — Task id (`T-NNN`) — a live TODO.md/BACKLOG.md line, children included.

###### **Options:**

* `--delta <N>` — Sibling steps — `-1` up, `+1` down; larger values walk that far, stopping at the edge.
* `--json` — Emit JSON (accepted everywhere; default output for machines).
* `--no-commit` — Skip the per-verb commit (also suppressed by $MYCO_NO_COMMIT).
* `--dir <PATH>` — Project worktree to act on, overriding discovery.
* `--root <PATH>` — Repo root to act on, overriding discovery (the worktree is `<root>/project`).
* `-h, --help` — Print help

## `myco absorb`

Import work into the ledger — TODO/FIXME comments, Jira CSV exports, GitHub issue dumps.

**Usage:** `myco absorb [OPTIONS] <COMMAND>`

###### **Commands:**

* `comments` — Sweep code comments into the ledger via ripgrep (`rg` must be installed — respects `.gitignore`; `project/`, `docs/`, `target/` are excluded).
* `jira` — Import a Jira CSV export. `Done`/`Resolved` rows are skipped unless `--include-done` (created done + archived).
* `github` — Import a GitHub issues dump — a JSON array (`number`, `title`, `state`, `labels`, `assignee`, `body`) or the CSV equivalent. `closed` rows are skipped unless `--include-done` (created done + archived).

###### **Options:**

* `--json` — Emit JSON (accepted everywhere; default output for machines).
* `--no-commit` — Skip the per-verb commit (also suppressed by $MYCO_NO_COMMIT).
* `--dir <PATH>` — Project worktree to act on, overriding discovery.
* `--root <PATH>` — Repo root to act on, overriding discovery (the worktree is `<root>/project`).
* `-h, --help` — Print help

## `myco absorb comments`

Sweep code comments into the ledger via ripgrep (`rg` must be installed — respects `.gitignore`; `project/`, `docs/`, `target/` are excluded).

**Usage:** `myco absorb comments [OPTIONS]`

###### **Options:**

* `--json` — Emit JSON (accepted everywhere; default output for machines).
* `--path <path>` — Directory to scan (default: the repo root).
* `--kinds <kinds>` — Comment kinds to match — comma-separated or repeatable (default TODO,FIXME,HACK).
* `--no-commit` — Skip the per-verb commit (also suppressed by $MYCO_NO_COMMIT).
* `--dir <PATH>` — Project worktree to act on, overriding discovery.
* `--to <to>` — Where findings land: `backlog` items or `issues` files (default `backlog`). [default: backlog]
* `--dry` — Report findings without creating items or committing.
* `--root <PATH>` — Repo root to act on, overriding discovery (the worktree is `<root>/project`).
* `-h, --help` — Print help

## `myco absorb jira`

Import a Jira CSV export. `Done`/`Resolved` rows are skipped unless `--include-done` (created done + archived).

**Usage:** `myco absorb jira [OPTIONS] <file>`

###### **Arguments:**

* `<file>` — The export file (`.csv`).

###### **Options:**

* `--include-done` — Import done/resolved issues too.
* `--json` — Emit JSON (accepted everywhere; default output for machines).
* `--no-commit` — Skip the per-verb commit (also suppressed by $MYCO_NO_COMMIT).
* `--dir <PATH>` — Project worktree to act on, overriding discovery.
* `--root <PATH>` — Repo root to act on, overriding discovery (the worktree is `<root>/project`).
* `-h, --help` — Print help

## `myco absorb github`

Import a GitHub issues dump — a JSON array (`number`, `title`, `state`, `labels`, `assignee`, `body`) or the CSV equivalent. `closed` rows are skipped unless `--include-done` (created done + archived).

**Usage:** `myco absorb github [OPTIONS] <file>`

###### **Arguments:**

* `<file>` — The export file (`.json` or `.csv`).

###### **Options:**

* `--include-done` — Import closed issues too.
* `--json` — Emit JSON (accepted everywhere; default output for machines).
* `--no-commit` — Skip the per-verb commit (also suppressed by $MYCO_NO_COMMIT).
* `--dir <PATH>` — Project worktree to act on, overriding discovery.
* `--root <PATH>` — Repo root to act on, overriding discovery (the worktree is `<root>/project`).
* `-h, --help` — Print help

## `myco index`

The derived index: register repos, query/rebuild snapshots.

**Usage:** `myco index [OPTIONS] <COMMAND>`

###### **Commands:**

* `add` — Register a repo — `<path>` is a repo root (or its `project/` worktree). Needed outside a lasius workspace (clones under `lasius.toml`'s `repos_dir` join the index universe automatically); inside one it still records an explicit entry. Ledger-less repos register as `ledger = "none"` with no snapshot; `--org` marks the org/global ledger (equivalent to `[index] ledger = "org"]` in the repo's `myco.toml`).
* `remove` — Unregister a repo and drop its snapshot.
* `list` — List registered repos with snapshot freshness.
* `rebuild` — Re-scan a registered repo (all of them without `<path>`).

###### **Options:**

* `--json` — Emit JSON (accepted everywhere; default output for machines).
* `--no-commit` — Skip the per-verb commit (also suppressed by $MYCO_NO_COMMIT).
* `--dir <PATH>` — Project worktree to act on, overriding discovery.
* `--root <PATH>` — Repo root to act on, overriding discovery (the worktree is `<root>/project`).
* `-h, --help` — Print help

## `myco index add`

Register a repo — `<path>` is a repo root (or its `project/` worktree). Needed outside a lasius workspace (clones under `lasius.toml`'s `repos_dir` join the index universe automatically); inside one it still records an explicit entry. Ledger-less repos register as `ledger = "none"` with no snapshot; `--org` marks the org/global ledger (equivalent to `[index] ledger = "org"]` in the repo's `myco.toml`).

**Usage:** `myco index add [OPTIONS] <path>`

###### **Arguments:**

* `<path>` — Repo root (or project worktree) to register.

###### **Options:**

* `--json` — Emit JSON (accepted everywhere; default output for machines).
* `--org` — Designate this repo as the org/global ledger — the fallback target verbs in ledger-less universe repos resolve to (at most one; clears the mark elsewhere). `[index] ledger = "org"]` in the repo's myco.toml is the durable equivalent.
* `--no-commit` — Skip the per-verb commit (also suppressed by $MYCO_NO_COMMIT).
* `--dir <PATH>` — Project worktree to act on, overriding discovery.
* `--root <PATH>` — Repo root to act on, overriding discovery (the worktree is `<root>/project`).
* `-h, --help` — Print help

## `myco index remove`

Unregister a repo and drop its snapshot.

**Usage:** `myco index remove [OPTIONS] <path>`

###### **Arguments:**

* `<path>` — Registered repo root (or project worktree).

###### **Options:**

* `--json` — Emit JSON (accepted everywhere; default output for machines).
* `--no-commit` — Skip the per-verb commit (also suppressed by $MYCO_NO_COMMIT).
* `--dir <PATH>` — Project worktree to act on, overriding discovery.
* `--root <PATH>` — Repo root to act on, overriding discovery (the worktree is `<root>/project`).
* `-h, --help` — Print help

## `myco index list`

List registered repos with snapshot freshness.

**Usage:** `myco index list [OPTIONS]`

###### **Options:**

* `--json` — Emit JSON (accepted everywhere; default output for machines).
* `--no-commit` — Skip the per-verb commit (also suppressed by $MYCO_NO_COMMIT).
* `--dir <PATH>` — Project worktree to act on, overriding discovery.
* `--root <PATH>` — Repo root to act on, overriding discovery (the worktree is `<root>/project`).
* `-h, --help` — Print help

## `myco index rebuild`

Re-scan a registered repo (all of them without `<path>`).

**Usage:** `myco index rebuild [OPTIONS] [path]`

###### **Arguments:**

* `[path]` — Registered repo root (or project worktree).

###### **Options:**

* `--json` — Emit JSON (accepted everywhere; default output for machines).
* `--no-commit` — Skip the per-verb commit (also suppressed by $MYCO_NO_COMMIT).
* `--dir <PATH>` — Project worktree to act on, overriding discovery.
* `--root <PATH>` — Repo root to act on, overriding discovery (the worktree is `<root>/project`).
* `-h, --help` — Print help

## `myco org`

Cross-repo scaffolding — re-run `init` on every registered repo. Cross-repo *views* are `myco view org-board|org-status` (index-sourced).

**Usage:** `myco org [OPTIONS] <COMMAND>`

###### **Commands:**

* `init` — Re-run `init` on every registered repo — refreshes the worktree scaffold and root files (.gitignore, the AGENTS.md agent-awareness block). The rollout path for new scaffolding.

###### **Options:**

* `--json` — Emit JSON (accepted everywhere; default output for machines).
* `--no-commit` — Skip the per-verb commit (also suppressed by $MYCO_NO_COMMIT).
* `--dir <PATH>` — Project worktree to act on, overriding discovery.
* `--root <PATH>` — Repo root to act on, overriding discovery (the worktree is `<root>/project`).
* `-h, --help` — Print help

## `myco org init`

Re-run `init` on every registered repo — refreshes the worktree scaffold and root files (.gitignore, the AGENTS.md agent-awareness block). The rollout path for new scaffolding.

**Usage:** `myco org init [OPTIONS]`

###### **Options:**

* `--json` — Emit JSON (accepted everywhere; default output for machines).
* `--no-commit` — Skip the per-verb commit (also suppressed by $MYCO_NO_COMMIT).
* `--dir <PATH>` — Project worktree to act on, overriding discovery.
* `--root <PATH>` — Repo root to act on, overriding discovery (the worktree is `<root>/project`).
* `-h, --help` — Print help

## `myco manifest`

Capabilities + protocol version — agent self-discovery.

**Usage:** `myco manifest [OPTIONS]`

###### **Options:**

* `--json` — Emit JSON (accepted everywhere; default output for machines).
* `--no-commit` — Skip the per-verb commit (also suppressed by $MYCO_NO_COMMIT).
* `--dir <PATH>` — Project worktree to act on, overriding discovery.
* `--root <PATH>` — Repo root to act on, overriding discovery (the worktree is `<root>/project`).
* `-h, --help` — Print help

## `myco schema`

The file/state spec, machine-readable JSON Schema.

**Usage:** `myco schema [OPTIONS]`

###### **Options:**

* `--json` — Emit JSON (accepted everywhere; default output for machines).
* `--no-commit` — Skip the per-verb commit (also suppressed by $MYCO_NO_COMMIT).
* `--dir <PATH>` — Project worktree to act on, overriding discovery.
* `--root <PATH>` — Repo root to act on, overriding discovery (the worktree is `<root>/project`).
* `-h, --help` — Print help

## `myco init`

Scaffold `project/`: orphan branch + worktree + .gitignore + seed files + the managed AGENTS.md agent-awareness block. Idempotent — existing worktree and AGENTS.md prose are preserved, only the marked block is (re)written.

**Usage:** `myco init [OPTIONS]`

###### **Options:**

* `--json` — Emit JSON (accepted everywhere; default output for machines).
* `--no-commit` — Skip the per-verb commit (also suppressed by $MYCO_NO_COMMIT).
* `--dir <PATH>` — Project worktree to act on, overriding discovery.
* `--root <PATH>` — Repo root to act on, overriding discovery (the worktree is `<root>/project`).
* `-h, --help` — Print help

## `myco check`

Validate the ledger (schemas, state machine, ids, refs) and the worktree it lives in (detached HEAD, wrong branch, unpushed commits); also validates the managed AGENTS.md block on the repo root. Findings carry a severity: `error` exits 1, `warning` is reported and costs nothing (ADR-0002). `--fix` applies safe repairs + commits, including reattaching a detached worktree.

**Usage:** `myco check [OPTIONS]`

###### **Options:**

* `--fix` — Repair drift in place instead of only reporting.
* `--json` — Emit JSON (accepted everywhere; default output for machines).
* `--no-commit` — Skip the per-verb commit (also suppressed by $MYCO_NO_COMMIT).
* `--dir <PATH>` — Project worktree to act on, overriding discovery.
* `--root <PATH>` — Repo root to act on, overriding discovery (the worktree is `<root>/project`).
* `-h, --help` — Print help

## `myco migrate`

Convert legacy ledgers to v2: `pm/` → `project/`, or normalize files already on the `project` branch.

**Usage:** `myco migrate [OPTIONS]`

###### **Options:**

* `--json` — Emit JSON (accepted everywhere; default output for machines).
* `--no-commit` — Skip the per-verb commit (also suppressed by $MYCO_NO_COMMIT).
* `--dir <PATH>` — Project worktree to act on, overriding discovery.
* `--root <PATH>` — Repo root to act on, overriding discovery (the worktree is `<root>/project`).
* `-h, --help` — Print help

## `myco commit`

Stage + commit worktree changes (conventional message).

**Usage:** `myco commit [OPTIONS]`

###### **Options:**

* `--json` — Emit JSON (accepted everywhere; default output for machines).
* `-m, --message <message>` — Commit message (default: `chore: update project ledger`).
* `--no-commit` — Skip the per-verb commit (also suppressed by $MYCO_NO_COMMIT).
* `--dir <PATH>` — Project worktree to act on, overriding discovery.
* `--root <PATH>` — Repo root to act on, overriding discovery (the worktree is `<root>/project`).
* `-h, --help` — Print help

## `myco undo`

Undo the last ledger commit — resets the `project` branch tip one back (`HEAD~1`). One verb = one commit, so the reset is the inverse mutation, file deletions included; the undone commit is journaled for `redo`. Refuses on a dirty worktree (uncommitted `--no-commit` writes), at the root commit, and when the tip is already published on `origin` (unless `--force`). `--list` shows the redoable stack without mutating.

**Usage:** `myco undo [OPTIONS]`

###### **Options:**

* `--force` — Undo even when the tip commit is already published on the remote.
* `--json` — Emit JSON (accepted everywhere; default output for machines).
* `--list` — List the redoable commits (newest first) instead of undoing.
* `--no-commit` — Skip the per-verb commit (also suppressed by $MYCO_NO_COMMIT).
* `--dir <PATH>` — Project worktree to act on, overriding discovery.
* `--root <PATH>` — Repo root to act on, overriding discovery (the worktree is `<root>/project`).
* `-h, --help` — Print help

## `myco redo`

Redo the last undo — pops the undo journal and restores the commit `myco undo` moved away from (`git reset --hard <target>`). Repeatable while entries remain; the stack self-invalidates when the ledger moves (any commit/pull/manual git op since the undo). Refuses on a dirty worktree.

**Usage:** `myco redo [OPTIONS]`

###### **Options:**

* `--json` — Emit JSON (accepted everywhere; default output for machines).
* `--no-commit` — Skip the per-verb commit (also suppressed by $MYCO_NO_COMMIT).
* `--dir <PATH>` — Project worktree to act on, overriding discovery.
* `--root <PATH>` — Repo root to act on, overriding discovery (the worktree is `<root>/project`).
* `-h, --help` — Print help

## `myco sync`

Reconcile the ledger branch with `origin/<branch>` — fetch, fast-forward or rebase local commits onto the remote tip, then heal merge damage: `meta.toml` counters reconcile, duplicate ids renumber remote-wins (the pushed item keeps its id; the local one moves to a fresh id, inbound `blocks:`/`links:`/supersede refs rebind to it, and a `renumbered` handoff entry records the audit). Mutating verbs refuse while behind (`ensure_synced`; `sync.auto_fetch` config fetches first), so this is the gate. A rebase conflict stops mid-flight — resolve + `git rebase --continue` + `myco sync` again, or `--abort` to bail. With `sync.auto_push` enabled (the default; `false` opts out), a successful sync also pushes the reconciled tip (best-effort — a push failure warns, never fails).

**Usage:** `myco sync [OPTIONS]`

###### **Options:**

* `--abort` — Abort a stopped rebase/merge (`git rebase --abort`) and return to the pre-sync tip.
* `--json` — Emit JSON (accepted everywhere; default output for machines).
* `--no-commit` — Skip the per-verb commit (also suppressed by $MYCO_NO_COMMIT).
* `--dir <PATH>` — Project worktree to act on, overriding discovery.
* `--root <PATH>` — Repo root to act on, overriding discovery (the worktree is `<root>/project`).
* `-h, --help` — Print help

## `myco view`

Declarative views — named presets (`[views.<name>]` in config + built-ins `board|open|epic|milestone|next|roadmap|stats|burnup`, plus index-sourced `org-board|org-status` for cross-repo) or an ad-hoc spec from flags (`--source index` unions the index universe — lasius clones ∪ registered repos). `myco view board` is the kanban; `format = "repos"` emits one row per repo; `format = "timeline"|"stats"|"burnup"` are the analytics emits (roadmap lanes, count tallies, done-over-time — `done:` archive stamps drive burnup); bare `myco view` lists views (or renders `default_view` when config names one).

**Usage:** `myco view [OPTIONS] [name]`

###### **Arguments:**

* `[name]` — View name — builtin preset or config view.

###### **Options:**

* `--json` — Emit JSON (accepted everywhere; default output for machines).
* `--list` — List defined views and exit.
* `--md` — Emit a markdown table (board/repos formats).
* `--no-commit` — Skip the per-verb commit (also suppressed by $MYCO_NO_COMMIT).
* `--all` — Show every row — no cap.
* `--dir <PATH>` — Project worktree to act on, overriding discovery.
* `--kind <kind>` — Item kinds — task|issue|proposal|adr|plan (repeatable/comma-separated).
* `--root <PATH>` — Repo root to act on, overriding discovery (the worktree is `<root>/project`).
* `--status <status>` — Statuses — open|claimed|in-progress|blocked|done|deferred|cancelled (repeatable/comma-separated).
* `--pri-min <pri_min>` — Inclusive lower severity bound — low|med|high|crit.
* `--pri-max <pri_max>` — Inclusive upper severity bound.
* `--label <label>` — `#`-less label — item carries any of these (repeatable/comma-separated).
* `--owner <owner>` — Owner actor — item's owner is any of these (repeatable/comma-separated).
* `--claimed` — Items with an owner.
* `--unclaimed` — Items with no owner.
* `--epic <epic>` — Items in this epic.
* `--milestone <milestone>` — Items in this milestone.
* `--due <due>` — Items due on this date (`YYYY-MM-DD`).
* `--est <est>` — Items with this point estimate. [alias: --estimate]
* `--cycle <cycle>` — Items in this cycle.
* `--live-only` — Items not superseded by another (ADRs).
* `--superseded` — Items superseded by another (ADRs).
* `--source <source>` — Item universe — repo (default) reads this worktree; index unions every registered repo's snapshot (org views).
* `--group-by <group_by>` — Group axis — status|priority|kind|owner|epic|milestone|repo|cycle.
* `--sort <sort>` — Sort keys — `pri,id,-created`; `-` flips a key's default direction.
* `--limit <N>` — Keep the first N items after sorting.
* `--format <format>` — Renderer — list|board|repos|timeline|stats|burnup.
* `-h, --help` — Print help

## `myco next`

The single highest-priority open item — "what should I work on" (a `view` eval with limit 1: open tasks+issues, `pri,id`; the filter flags merge over that default). `--claim` mutates it to `claimed` + an owner in one commit.

**Usage:** `myco next [OPTIONS]`

###### **Options:**

* `--json` — Emit JSON (accepted everywhere; default output for machines).
* `--kind <kind>` — Item kinds — task|issue|proposal|adr|plan (repeatable/comma-separated).
* `--no-commit` — Skip the per-verb commit (also suppressed by $MYCO_NO_COMMIT).
* `--status <status>` — Statuses — open|claimed|in-progress|blocked|done|deferred|cancelled (repeatable/comma-separated).
* `--dir <PATH>` — Project worktree to act on, overriding discovery.
* `--pri-min <pri_min>` — Inclusive lower severity bound — low|med|high|crit.
* `--pri-max <pri_max>` — Inclusive upper severity bound.
* `--root <PATH>` — Repo root to act on, overriding discovery (the worktree is `<root>/project`).
* `--label <label>` — `#`-less label — item carries any of these (repeatable/comma-separated).
* `--owner <owner>` — Owner actor — item's owner is any of these (repeatable/comma-separated).
* `--claimed` — Items with an owner.
* `--unclaimed` — Items with no owner.
* `--epic <epic>` — Items in this epic.
* `--milestone <milestone>` — Items in this milestone.
* `--due <due>` — Items due on this date (`YYYY-MM-DD`).
* `--est <est>` — Items with this point estimate. [alias: --estimate]
* `--cycle <cycle>` — Items in this cycle.
* `--live-only` — Items not superseded by another (ADRs).
* `--superseded` — Items superseded by another (ADRs).
* `--sort <sort>` — Sort keys — `pri,id,-created`; `-` flips a key's default direction. Default `pri,id`.
* `--claim` — Claim the emitted item — status `claimed`, owner set to the `--owner` actor (else the resolved actor); `--owner` names the claimer here and doesn't filter. Mutating: one commit.
* `--steal` — With `--claim`: take the item even when another actor owns the claim — records a `taken from` handoff entry.
* `--session <session>` — With `--claim`: tag the claim with a session id — appends a `claimed` handoff entry carrying it (`interrupt --session` scopes teardown to it).
* `-h, --help` — Print help

## `myco status`

Cheap digest of the work ledger. `--prompt` = token-sized.

**Usage:** `myco status [OPTIONS]`

###### **Options:**

* `--json` — Emit JSON (accepted everywhere; default output for machines).
* `--prompt` — Render for an agent prompt (minimal tokens).
* `--no-commit` — Skip the per-verb commit (also suppressed by $MYCO_NO_COMMIT).
* `--dir <PATH>` — Project worktree to act on, overriding discovery.
* `--root <PATH>` — Repo root to act on, overriding discovery (the worktree is `<root>/project`).
* `-h, --help` — Print help

## `myco report`

Aging + velocity + blocked — how the ledger is trending.

**Usage:** `myco report [OPTIONS]`

###### **Options:**

* `--json` — Emit JSON (accepted everywhere; default output for machines).
* `--no-commit` — Skip the per-verb commit (also suppressed by $MYCO_NO_COMMIT).
* `--dir <PATH>` — Project worktree to act on, overriding discovery.
* `--root <PATH>` — Repo root to act on, overriding discovery (the worktree is `<root>/project`).
* `-h, --help` — Print help

## `myco handoff`

Attach resumable state at session end — appends a dated entry to `handoffs/<ID>.md` (any item kind, live or archived; append-only).

**Usage:** `myco handoff [OPTIONS] --note <TEXT|-> <id>`

###### **Arguments:**

* `<id>` — Any item id (`T-NNN`, `I-NNN`, `P-NNN`, `ADR-NNNN`, `PL-NNN`).

###### **Options:**

* `--json` — Emit JSON (accepted everywhere; default output for machines).
* `--note <TEXT|->` — Handoff note — done/next/open. `-` reads stdin.
* `--actor <actor>` — Who wrote it (`devin`, `human:luis`, …; default: the resolved actor).
* `--no-commit` — Skip the per-verb commit (also suppressed by $MYCO_NO_COMMIT).
* `--dir <PATH>` — Project worktree to act on, overriding discovery.
* `--session <session>` — Session provenance — a sepia session id, `cli:<pid>`, any id string.
* `--root <PATH>` — Repo root to act on, overriding discovery (the worktree is `<root>/project`).
* `-h, --help` — Print help

## `myco interrupt`

A session ended with work still claimed/in-progress/blocked — revert the item to `open` + `interrupted` handoff, one commit. Without an id, `--session <id>` is the teardown: every item claimed under that session reverts in one commit. Session-lifecycle hooks (sepia, CLI session-end) call this; myco never detects sessions itself.

**Usage:** `myco interrupt [OPTIONS] [id]`

###### **Arguments:**

* `[id]` — Any item id (`T-NNN`, `I-NNN`, `P-NNN`, `ADR-NNNN`, `PL-NNN`) — omit for the `--session` bulk teardown.

###### **Options:**

* `--json` — Emit JSON (accepted everywhere; default output for machines).
* `--note <TEXT>` — Extra context appended to the `interrupted` note.
* `--actor <actor>` — Who interrupted it (default: the resolved actor).
* `--no-commit` — Skip the per-verb commit (also suppressed by $MYCO_NO_COMMIT).
* `--dir <PATH>` — Project worktree to act on, overriding discovery.
* `--session <session>` — Session scope — a sepia session id, `cli:<pid>`, any id string. With `id`: interrupt only when the item's claim tag matches. Without `id`: teardown — interrupt every item claimed under it.
* `--root <PATH>` — Repo root to act on, overriding discovery (the worktree is `<root>/project`).
* `-h, --help` — Print help

## `myco reap`

Bulk stale-claim reaper — every live `claimed`/`in-progress` item whose last activity (latest handoff entry, else `created`) is older than `--idle-days` reverts to `open` + an `interrupted` handoff, ONE commit for the whole sweep. `--session <id>` scopes to items whose claim tag names that session; `--dry-run` lists the sweep with zero writes. Scheduler-facing — peregrin campaigns call it periodically; the no-hook safety net behind `report`'s stale-claims finding.

**Usage:** `myco reap [OPTIONS]`

###### **Options:**

* `--idle-days <N>` — Idle threshold in days — last activity older than this reaps. Default: the report's stale-claim threshold (3).
* `--json` — Emit JSON (accepted everywhere; default output for machines).
* `--no-commit` — Skip the per-verb commit (also suppressed by $MYCO_NO_COMMIT).
* `--session <session>` — Scope to items whose claim tag names this session (`interrupt --session`'s rule).
* `--actor <actor>` — Who reaped it (default: the resolved actor).
* `--dir <PATH>` — Project worktree to act on, overriding discovery.
* `--dry-run` — List what would be reaped — no writes, no commit.
* `--root <PATH>` — Repo root to act on, overriding discovery (the worktree is `<root>/project`).
* `-h, --help` — Print help

## `myco resume`

Minimal context block to restart work on an item — the item, `blocks:` dep titles, latest handoffs, open checklist. The single-item `status --prompt`. 1..n ids batch in one call.

**Usage:** `myco resume [OPTIONS] <id>...`

###### **Arguments:**

* `<id>...` — Any item id (`T-NNN`, `I-NNN`, `P-NNN`, `ADR-NNNN`, `PL-NNN`) — live or archived.

###### **Options:**

* `--json` — Emit JSON (accepted everywhere; default output for machines).
* `--last <last>` — Only the latest N handoff entries (default 10).
* `--no-commit` — Skip the per-verb commit (also suppressed by $MYCO_NO_COMMIT).
* `--dir <PATH>` — Project worktree to act on, overriding discovery.
* `--root <PATH>` — Repo root to act on, overriding discovery (the worktree is `<root>/project`).
* `-h, --help` — Print help

## `myco ui`

Interactive board TUI (ratatui) — TODO/BACKLOG tabs, item preview, writes through the same ops the verbs use; auto-reloads when the ledger changes on disk. `--view` with a `source = "index"` spec (org-*) opens the cross-repo index board: `repo:ID` rows, verbs write the owning repo.

**Usage:** `myco ui [OPTIONS]`

###### **Options:**

* `--json` — Emit JSON (accepted everywhere; default output for machines).
* `--view <NAME>` — Open with a named view's filter narrowing every board; index-sourced views load the cross-repo universe.
* `--no-commit` — Skip the per-verb commit (also suppressed by $MYCO_NO_COMMIT).
* `--dir <PATH>` — Project worktree to act on, overriding discovery.
* `--root <PATH>` — Repo root to act on, overriding discovery (the worktree is `<root>/project`).
* `-h, --help` — Print help

## `myco serve`

Sequential request/response verb surface on stdio — JSON-RPC 2.0, one line in / one line out. For embedding (peregrin spawns one per repo); stdin IS the wire, so no served method reads content from it.

**Usage:** `myco serve [OPTIONS]`

###### **Options:**

* `--json` — Emit JSON (accepted everywhere; default output for machines).
* `--no-commit` — Skip the per-verb commit (also suppressed by $MYCO_NO_COMMIT).
* `--dir <PATH>` — Project worktree to act on, overriding discovery.
* `--root <PATH>` — Repo root to act on, overriding discovery (the worktree is `<root>/project`).
* `-h, --help` — Print help

## `myco batch`

One-shot `serve` — NDJSON verb requests from stdin or `--file`, one JSON-RPC response line per request, dispatched through the same served surface. Per-line errors never fail the run; mutations keep per-request commits (`--no-commit`/`$MYCO_NO_COMMIT` apply). stdin IS the wire here too — requests that would read it are rejected.

**Usage:** `myco batch [OPTIONS]`

###### **Options:**

* `--file <PATH>` — Read NDJSON requests from this file instead of stdin.
* `--json` — Emit JSON (accepted everywhere; default output for machines).
* `--no-commit` — Skip the per-verb commit (also suppressed by $MYCO_NO_COMMIT).
* `--dir <PATH>` — Project worktree to act on, overriding discovery.
* `--root <PATH>` — Repo root to act on, overriding discovery (the worktree is `<root>/project`).
* `-h, --help` — Print help
