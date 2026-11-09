# Command-Line Help for `myco`

This document contains the help content for the `myco` command-line program.

**Command Overview:**

* [`myco`↴](#myco)
* [`myco todo`↴](#myco-todo)
* [`myco todo add`↴](#myco-todo-add)
* [`myco todo done`↴](#myco-todo-done)
* [`myco todo defer`↴](#myco-todo-defer)
* [`myco todo list`↴](#myco-todo-list)
* [`myco backlog`↴](#myco-backlog)
* [`myco backlog add`↴](#myco-backlog-add)
* [`myco backlog promote`↴](#myco-backlog-promote)
* [`myco backlog list`↴](#myco-backlog-list)
* [`myco done`↴](#myco-done)
* [`myco done list`↴](#myco-done-list)
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
* [`myco view`↴](#myco-view)
* [`myco next`↴](#myco-next)
* [`myco status`↴](#myco-status)
* [`myco report`↴](#myco-report)
* [`myco handoff`↴](#myco-handoff)
* [`myco interrupt`↴](#myco-interrupt)
* [`myco resume`↴](#myco-resume)
* [`myco ui`↴](#myco-ui)
* [`myco serve`↴](#myco-serve)
* [`myco batch`↴](#myco-batch)

## `myco`

myco — agent-first project management for a repo's `project/` worktree.

**Usage:** `myco [OPTIONS] <COMMAND>`

###### **Subcommands:**

* `todo` — TODO.md tasks — add / done / defer / list.
* `backlog` — BACKLOG.md parked items — add / promote / list.
* `done` — DONE.md — the done-task archive `todo done` lands in.
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
* `view` — Declarative views — named presets (`[views.<name>]` in config + built-ins `board|open|epic|milestone`, plus index-sourced `org-board|org-status` for cross-repo) or an ad-hoc spec from flags (`--source index` unions the registered repos' snapshots). `myco view board` is the kanban; `format = "repos"` emits one row per repo; bare `myco view` lists views (or renders `default_view` when config names one).
* `next` — The single highest-priority open item — "what should I work on" (a `view` eval with limit 1: open tasks+issues, `pri,id`; the filter flags merge over that default). `--claim` mutates it to `claimed` + an owner in one commit.
* `status` — Cheap digest of the work ledger. `--prompt` = token-sized.
* `report` — Aging + velocity + blocked — how the ledger is trending.
* `handoff` — Attach resumable state at session end — appends a dated entry to `handoffs/<ID>.md` (any item kind, live or archived; append-only).
* `interrupt` — A session ended with work still claimed/in-progress/blocked — revert the item to `open` + `interrupted` handoff, one commit. Without an id, `--session <id>` is the teardown: every item claimed under that session reverts in one commit. Session-lifecycle hooks (sepia, CLI session-end) call this; myco never detects sessions itself.
* `resume` — Minimal context block to restart work on an item — the item, `blocks:` dep titles, latest handoffs, open checklist. The single-item `status --prompt`. 1..n ids batch in one call.
* `ui` — Interactive board TUI (ratatui) — TODO/BACKLOG tabs, item preview, writes through the same ops the verbs use; auto-reloads when the ledger changes on disk. `--view` with a `source = "index"` spec (org-*) opens the cross-repo index board: `repo:ID` rows, verbs write the owning repo.
* `serve` — Sequential request/response verb surface on stdio — JSON-RPC 2.0, one line in / one line out. For embedding (peregrin spawns one per repo); stdin IS the wire, so no served method reads content from it.
* `batch` — One-shot `serve` — NDJSON verb requests from stdin or `--file`, one JSON-RPC response line per request, dispatched through the same served surface. Per-line errors never fail the run; mutations keep per-request commits (`--no-commit`/`$MYCO_NO_COMMIT` apply). stdin IS the wire here too — requests that would read it are rejected.

###### **Options:**

* `--json` — Emit JSON (accepted everywhere; default output for machines).
* `--no-commit` — Skip the per-verb commit (also suppressed by $MYCO_NO_COMMIT).
* `--dir <PATH>` — Project worktree to act on, overriding discovery.
* `--root <PATH>` — Repo root to act on, overriding discovery (the worktree is `<root>/project`).



## `myco todo`

TODO.md tasks — add / done / defer / list.

**Usage:** `myco todo <COMMAND>`

###### **Subcommands:**

* `add` — Add a task — allocs a T-id, appends to a section of TODO.md.
* `done` — Mark done — walks the state machine to `done`, removes the line, and archives the subtree into DONE.md. Done on a parent archives the whole subtree — children die with it. Confirms on a tty (`--yes`/`$MYCO_YES` skips; scripts never prompt).
* `defer` — Park a task — transition to `deferred`, move the line to BACKLOG.md. Defer on a parent moves the whole subtree (children park with it); on a child it detaches alone.
* `list` — List tasks.



## `myco todo add`

Add a task — allocs a T-id, appends to a section of TODO.md.

**Usage:** `myco todo add [OPTIONS] <title>`

###### **Arguments:**

* `<TITLE>` — One-line title.

###### **Options:**

* `--section <SECTION>` — Section to add under (created when absent; default: the first section, or an unsectioned run).
* `--parent <PARENT>` — Nest under this task instead of a section — the new item becomes a child (`  - [ ]` under the parent).
* `--pri <PRI>` [alias: `priority`] — Priority: low|med|high|crit.
* `--owner <OWNER>` — Owner actor id (`devin`, `human:luis`, …).
* `--label <LABEL>` — `#label` — repeatable.
* `--blocks <BLOCKS>` — Ids this item blocks — comma-separated or repeatable.
* `--epic <EPIC>` — Epic name — the delivery grouping this item belongs to (`[A-Za-z0-9_.-]+`).
* `--milestone <MILESTONE>` — Milestone name — the target this item ships under (`[A-Za-z0-9_.-]+`).



## `myco todo done`

Mark done — walks the state machine to `done`, removes the line, and archives the subtree into DONE.md. Done on a parent archives the whole subtree — children die with it. Confirms on a tty (`--yes`/`$MYCO_YES` skips; scripts never prompt).

**Usage:** `myco todo done [OPTIONS] <id>`

###### **Arguments:**

* `<ID>` — Task id (`T-NNN`).

###### **Options:**

* `--yes` — Skip the confirmation prompt (also `$MYCO_YES`; non-interactive runs never prompt).



## `myco todo defer`

Park a task — transition to `deferred`, move the line to BACKLOG.md. Defer on a parent moves the whole subtree (children park with it); on a child it detaches alone.

**Usage:** `myco todo defer <id>`

###### **Arguments:**

* `<ID>` — Task id (`T-NNN`).



## `myco todo list`

List tasks.

**Usage:** `myco todo list [OPTIONS]`

###### **Options:**

* `--status <STATUS>` — Keep only items with this status.
* `--owner <OWNER>` — Keep only items owned by this actor.
* `--label <LABEL>` — Keep only items carrying all of these labels (repeatable).
* `--pri <PRI>` — Keep only items at this priority.
* `--epic <EPIC>` — Keep only items in this epic.
* `--milestone <MILESTONE>` — Keep only items in this milestone.
* `--query <QUERY>` — Keep only items whose id, title, or a label contains this text (case-insensitive).
* `--limit <N>` — Keep only the first N items after filtering.



## `myco backlog`

BACKLOG.md parked items — add / promote / list.

**Usage:** `myco backlog <COMMAND>`

###### **Subcommands:**

* `add` — Add a deferred item — allocs a T-id straight into BACKLOG.md.
* `promote` — Re-activate a parked item — `deferred → open`, move the line to TODO.md.
* `list` — List backlog items.



## `myco backlog add`

Add a deferred item — allocs a T-id straight into BACKLOG.md.

**Usage:** `myco backlog add [OPTIONS] <title>`

###### **Arguments:**

* `<TITLE>` — One-line title.

###### **Options:**

* `--section <SECTION>` — Section to add under (created when absent; default: the first section, or an unsectioned run).
* `--pri <PRI>` [alias: `priority`] — Priority: low|med|high|crit.
* `--owner <OWNER>` — Owner actor id (`devin`, `human:luis`, …).
* `--label <LABEL>` — `#label` — repeatable.
* `--blocks <BLOCKS>` — Ids this item blocks — comma-separated or repeatable.
* `--epic <EPIC>` — Epic name — the delivery grouping this item belongs to (`[A-Za-z0-9_.-]+`).
* `--milestone <MILESTONE>` — Milestone name — the target this item ships under (`[A-Za-z0-9_.-]+`).



## `myco backlog promote`

Re-activate a parked item — `deferred → open`, move the line to TODO.md.

**Usage:** `myco backlog promote <id>`

###### **Arguments:**

* `<ID>` — Task id (`T-NNN`).



## `myco backlog list`

List backlog items.

**Usage:** `myco backlog list [OPTIONS]`

###### **Options:**

* `--status <STATUS>` — Keep only items with this status.
* `--owner <OWNER>` — Keep only items owned by this actor.
* `--label <LABEL>` — Keep only items carrying all of these labels (repeatable).
* `--pri <PRI>` — Keep only items at this priority.
* `--epic <EPIC>` — Keep only items in this epic.
* `--milestone <MILESTONE>` — Keep only items in this milestone.
* `--query <QUERY>` — Keep only items whose id, title, or a label contains this text (case-insensitive).
* `--limit <N>` — Keep only the first N items after filtering.



## `myco done`

DONE.md — the done-task archive `todo done` lands in.

**Usage:** `myco done <COMMAND>`

###### **Subcommands:**

* `list` — List done tasks — the DONE.md archive `todo done`/`item set --status done` land in. Read-only; done is terminal.



## `myco done list`

List done tasks — the DONE.md archive `todo done`/`item set --status done` land in. Read-only; done is terminal.

**Usage:** `myco done list [OPTIONS]`

###### **Options:**

* `--status <STATUS>` — Keep only items with this status.
* `--owner <OWNER>` — Keep only items owned by this actor.
* `--label <LABEL>` — Keep only items carrying all of these labels (repeatable).
* `--pri <PRI>` — Keep only items at this priority.
* `--epic <EPIC>` — Keep only items in this epic.
* `--milestone <MILESTONE>` — Keep only items in this milestone.
* `--query <QUERY>` — Keep only items whose id, title, or a label contains this text (case-insensitive).
* `--limit <N>` — Keep only the first N items after filtering.



## `myco issue`

Tracked issues, one file each under `issues/`.

**Usage:** `myco issue <COMMAND>`

###### **Subcommands:**

* `new` — File an issue — allocs an I-id, writes `issues/issue-NNNN-<slug>.md` with frontmatter + template.
* `list` — List issues (live files; `--archived` includes `issues/archive/`).
* `resolve` — Close an issue — walks the state machine to `done`; `--resolution` appends text under `## Resolution`.
* `archive` — Shelve a done issue — moves the file to `issues/archive/`.
* `comment` — Append a dated note under `## Comments` (created when missing).



## `myco issue new`

File an issue — allocs an I-id, writes `issues/issue-NNNN-<slug>.md` with frontmatter + template.

**Usage:** `myco issue new [OPTIONS] <title>`

###### **Arguments:**

* `<TITLE>` — One-line title (drives the filename slug).

###### **Options:**

* `--body <FILE|->` — Body markdown from a file, or `-` for stdin.
* `--text <TEXT>` — Body markdown inline.
* `--pri <PRI>` [alias: `priority`] — Priority: low|med|high|crit.
* `--owner <OWNER>` — Owner actor id (`devin`, `human:luis`, …).
* `--label <LABEL>` — `#label` — repeatable.
* `--blocks <BLOCKS>` — Ids this item blocks — comma-separated or repeatable.
* `--epic <EPIC>` — Epic name — the delivery grouping this item belongs to (`[A-Za-z0-9_.-]+`).
* `--milestone <MILESTONE>` — Milestone name — the target this item ships under (`[A-Za-z0-9_.-]+`).



## `myco issue list`

List issues (live files; `--archived` includes `issues/archive/`).

**Usage:** `myco issue list [OPTIONS]`

###### **Options:**

* `--archived` — Include archived items.
* `--status <STATUS>` — Keep only items with this status.
* `--owner <OWNER>` — Keep only items owned by this actor.
* `--label <LABEL>` — Keep only items carrying all of these labels (repeatable).
* `--pri <PRI>` — Keep only items at this priority.
* `--epic <EPIC>` — Keep only items in this epic.
* `--milestone <MILESTONE>` — Keep only items in this milestone.
* `--query <QUERY>` — Keep only items whose id, title, or a label contains this text (case-insensitive).
* `--limit <N>` — Keep only the first N items after filtering.



## `myco issue resolve`

Close an issue — walks the state machine to `done`; `--resolution` appends text under `## Resolution`.

**Usage:** `myco issue resolve [OPTIONS] <id>`

###### **Arguments:**

* `<ID>` — Issue id (`I-NNN`).

###### **Options:**

* `--resolution <RESOLUTION>` — Resolution text appended to the body.



## `myco issue archive`

Shelve a done issue — moves the file to `issues/archive/`.

**Usage:** `myco issue archive <id>`

###### **Arguments:**

* `<ID>` — Issue id (`I-NNN`).



## `myco issue comment`

Append a dated note under `## Comments` (created when missing).

**Usage:** `myco issue comment <id> <text>`

###### **Arguments:**

* `<ID>` — Issue id (`I-NNN`).
* `<TEXT>` — Comment text.



## `myco proposal`

Design proposals under `proposals/` (`P-NNN`).

**Usage:** `myco proposal <COMMAND>`

###### **Subcommands:**

* `new` — File a proposal — allocs a P-id, writes the proposal file with frontmatter + template.
* `list` — List proposals.
* `status` — Set a proposal's status. Proposal words alias the state machine: `draft` = open, `accepted` = done, `rejected` = deferred — the transition is walked through the machine, so `accepted` passes `open → in-progress → done`.



## `myco proposal new`

File a proposal — allocs a P-id, writes the proposal file with frontmatter + template.

**Usage:** `myco proposal new [OPTIONS] <title>`

###### **Arguments:**

* `<TITLE>` — One-line title (drives the filename slug).

###### **Options:**

* `--body <FILE|->` — Body markdown from a file, or `-` for stdin.
* `--text <TEXT>` — Body markdown inline.
* `--pri <PRI>` [alias: `priority`] — Priority: low|med|high|crit.
* `--owner <OWNER>` — Owner actor id (`devin`, `human:luis`, …).
* `--label <LABEL>` — `#label` — repeatable.
* `--blocks <BLOCKS>` — Ids this item blocks — comma-separated or repeatable.
* `--epic <EPIC>` — Epic name — the delivery grouping this item belongs to (`[A-Za-z0-9_.-]+`).
* `--milestone <MILESTONE>` — Milestone name — the target this item ships under (`[A-Za-z0-9_.-]+`).



## `myco proposal list`

List proposals.

**Usage:** `myco proposal list [OPTIONS]`

###### **Options:**

* `--status <STATUS>` — Keep only items with this status.
* `--owner <OWNER>` — Keep only items owned by this actor.
* `--label <LABEL>` — Keep only items carrying all of these labels (repeatable).
* `--pri <PRI>` — Keep only items at this priority.
* `--epic <EPIC>` — Keep only items in this epic.
* `--milestone <MILESTONE>` — Keep only items in this milestone.
* `--query <QUERY>` — Keep only items whose id, title, or a label contains this text (case-insensitive).
* `--limit <N>` — Keep only the first N items after filtering.



## `myco proposal status`

Set a proposal's status. Proposal words alias the state machine: `draft` = open, `accepted` = done, `rejected` = deferred — the transition is walked through the machine, so `accepted` passes `open → in-progress → done`.

**Usage:** `myco proposal status <id> <status>`

###### **Arguments:**

* `<ID>` — Proposal id (`P-NNN`).
* `<STATUS>` — `draft` | `accepted` | `rejected` (raw machine statuses like `in-progress` also accepted).



## `myco plan`

Execution plans under `plans/` (`PL-NNN`).

**Usage:** `myco plan <COMMAND>`

###### **Subcommands:**

* `new` — File a plan — allocs a PL-id, writes the plan file with frontmatter + a phase-checklist template.
* `list` — List plans.



## `myco plan new`

File a plan — allocs a PL-id, writes the plan file with frontmatter + a phase-checklist template.

**Usage:** `myco plan new [OPTIONS] <title>`

###### **Arguments:**

* `<TITLE>` — One-line title (drives the filename slug).

###### **Options:**

* `--body <FILE|->` — Body markdown from a file, or `-` for stdin.
* `--text <TEXT>` — Body markdown inline.
* `--pri <PRI>` [alias: `priority`] — Priority: low|med|high|crit.
* `--owner <OWNER>` — Owner actor id (`devin`, `human:luis`, …).
* `--label <LABEL>` — `#label` — repeatable.
* `--blocks <BLOCKS>` — Ids this item blocks — comma-separated or repeatable.
* `--epic <EPIC>` — Epic name — the delivery grouping this item belongs to (`[A-Za-z0-9_.-]+`).
* `--milestone <MILESTONE>` — Milestone name — the target this item ships under (`[A-Za-z0-9_.-]+`).



## `myco plan list`

List plans.

**Usage:** `myco plan list [OPTIONS]`

###### **Options:**

* `--status <STATUS>` — Keep only items with this status.
* `--owner <OWNER>` — Keep only items owned by this actor.
* `--label <LABEL>` — Keep only items carrying all of these labels (repeatable).
* `--pri <PRI>` — Keep only items at this priority.
* `--epic <EPIC>` — Keep only items in this epic.
* `--milestone <MILESTONE>` — Keep only items in this milestone.
* `--query <QUERY>` — Keep only items whose id, title, or a label contains this text (case-insensitive).
* `--limit <N>` — Keep only the first N items after filtering.



## `myco adr`

Architecture decision records under `adrs/` (`ADR-NNNN`).

**Usage:** `myco adr <COMMAND>`

###### **Subcommands:**

* `new` — File an ADR — allocs an ADR-id, writes the file with frontmatter (status `open` = "proposed") + template.
* `list` — List ADRs.
* `supersede` — Mark an ADR superseded by another — the old file gets `superseded_by:` + status `done`, the replacement gets a reciprocal `supersedes:` key. Both must already exist.



## `myco adr new`

File an ADR — allocs an ADR-id, writes the file with frontmatter (status `open` = "proposed") + template.

**Usage:** `myco adr new [OPTIONS] <title>`

###### **Arguments:**

* `<TITLE>` — One-line title (drives the filename slug).

###### **Options:**

* `--body <FILE|->` — Body markdown from a file, or `-` for stdin.
* `--text <TEXT>` — Body markdown inline.
* `--pri <PRI>` [alias: `priority`] — Priority: low|med|high|crit.
* `--owner <OWNER>` — Owner actor id (`devin`, `human:luis`, …).
* `--label <LABEL>` — `#label` — repeatable.
* `--blocks <BLOCKS>` — Ids this item blocks — comma-separated or repeatable.
* `--epic <EPIC>` — Epic name — the delivery grouping this item belongs to (`[A-Za-z0-9_.-]+`).
* `--milestone <MILESTONE>` — Milestone name — the target this item ships under (`[A-Za-z0-9_.-]+`).



## `myco adr list`

List ADRs.

**Usage:** `myco adr list [OPTIONS]`

###### **Options:**

* `--status <STATUS>` — Keep only items with this status.
* `--owner <OWNER>` — Keep only items owned by this actor.
* `--label <LABEL>` — Keep only items carrying all of these labels (repeatable).
* `--pri <PRI>` — Keep only items at this priority.
* `--epic <EPIC>` — Keep only items in this epic.
* `--milestone <MILESTONE>` — Keep only items in this milestone.
* `--query <QUERY>` — Keep only items whose id, title, or a label contains this text (case-insensitive).
* `--limit <N>` — Keep only the first N items after filtering.



## `myco adr supersede`

Mark an ADR superseded by another — the old file gets `superseded_by:` + status `done`, the replacement gets a reciprocal `supersedes:` key. Both must already exist.

**Usage:** `myco adr supersede --by <by> <id>`

###### **Arguments:**

* `<ID>` — The ADR being replaced (`ADR-NNNN`).

###### **Options:**

* `--by <BY>` — The ADR replacing it (`ADR-NNNN`).



## `myco item`

Kind-agnostic item verbs — show / set / move.

**Usage:** `myco item <COMMAND>`

###### **Subcommands:**

* `show` — Dump items — frontmatter fields, kind-grouped related edges (blocks/blocked_by/supersedes/references/handoff, live-derived), plus the body for file-backed kinds (`--json` = the full Item + location + related + body). 1..n ids: a batch repeats the block in argument order; `--json` emits the per-id `[{id, ok, …}]` array.
* `set` — Mutate an item in place. `--status` is the only field routed through the state machine — single steps only, invalid ones error with the valid targets. Status changes on tasks move the line between TODO.md and BACKLOG.md (`done` archives the subtree into DONE.md — terminal, so it confirms on a tty: `--yes`/`$MYCO_YES` skips).
* `move` — Move an item to another repo's ledger — re-creates it in the destination's own id sequence (kind, title, body, pri/owner/labels/epic/milestone carried; `blocks:` dropped — it names source-ledger ids), writes a `moved from`/`moved to` provenance handoff on both sides, then walks the source to `done` (a task's line leaves the ledger; its handoff drops to `handoffs/archive/`). `--to` takes the destination repo root, its `project/` worktree, or a registered repo name (`myco index list`). Two commits — one per worktree.



## `myco item show`

Dump items — frontmatter fields, kind-grouped related edges (blocks/blocked_by/supersedes/references/handoff, live-derived), plus the body for file-backed kinds (`--json` = the full Item + location + related + body). 1..n ids: a batch repeats the block in argument order; `--json` emits the per-id `[{id, ok, …}]` array.

**Usage:** `myco item show <id>...`

###### **Arguments:**

* `<ID>` — Any item id (`T-NNN`, `I-NNN`, `P-NNN`, `ADR-NNNN`, `PL-NNN`) — live or archived.



## `myco item set`

Mutate an item in place. `--status` is the only field routed through the state machine — single steps only, invalid ones error with the valid targets. Status changes on tasks move the line between TODO.md and BACKLOG.md (`done` archives the subtree into DONE.md — terminal, so it confirms on a tty: `--yes`/`$MYCO_YES` skips).

**Usage:** `myco item set [OPTIONS] <id>`

###### **Arguments:**

* `<ID>` — Any item id.

###### **Options:**

* `--status <STATUS>` — New status — one legal step from the current one.
* `--pri <PRI>` [alias: `priority`] — Priority: low|med|high|crit.
* `--owner <OWNER>` — Owner actor id.
* `--label <LABEL>` — Add (`+x` or `x`) or remove (`-x`) a label — repeatable.
* `--unlabel <UNLABEL>` — Remove a label — repeatable.
* `--blocks <BLOCKS>` — Add (`+T-1` or `T-1`) or remove (`-T-1`) a blocks dep — repeatable or comma-separated.
* `--unblock <UNBLOCK>` — Remove a blocks dep — repeatable or comma-separated.
* `--epic <EPIC>` — Epic name — `""` clears the epic.
* `--milestone <MILESTONE>` — Milestone name — `""` clears the milestone.
* `--session <SESSION>` — Tag the item's claim with a session id — appends a `claimed` handoff entry carrying it (`interrupt --session` scopes teardown to it). The item must land in a work state.
* `--yes` — Skip the `--status done` confirmation prompt (also `$MYCO_YES`; non-interactive runs never prompt).



## `myco item move`

Move an item to another repo's ledger — re-creates it in the destination's own id sequence (kind, title, body, pri/owner/labels/epic/milestone carried; `blocks:` dropped — it names source-ledger ids), writes a `moved from`/`moved to` provenance handoff on both sides, then walks the source to `done` (a task's line leaves the ledger; its handoff drops to `handoffs/archive/`). `--to` takes the destination repo root, its `project/` worktree, or a registered repo name (`myco index list`). Two commits — one per worktree.

**Usage:** `myco item move [OPTIONS] --to <to> <id>`

###### **Arguments:**

* `<ID>` — Any item id.

###### **Options:**

* `--to <TO>` — Destination — a repo root, its `project/` worktree, or a registered repo name.
* `--note <NOTE>` — Extra context appended to the source's `moved to` handoff note.
* `--actor <ACTOR>` — Who moved it (default `cli`).
* `--yes` — Skip the confirmation prompt (also `$MYCO_YES`; non-interactive runs never prompt).



## `myco absorb`

Import work into the ledger — TODO/FIXME comments, Jira CSV exports, GitHub issue dumps.

**Usage:** `myco absorb <COMMAND>`

###### **Subcommands:**

* `comments` — Sweep code comments into the ledger via ripgrep (`rg` must be installed — respects `.gitignore`; `project/`, `docs/`, `target/` are excluded).
* `jira` — Import a Jira CSV export. `Done`/`Resolved` rows are skipped unless `--include-done` (created done + archived).
* `github` — Import a GitHub issues dump — a JSON array (`number`, `title`, `state`, `labels`, `assignee`, `body`) or the CSV equivalent. `closed` rows are skipped unless `--include-done` (created done + archived).



## `myco absorb comments`

Sweep code comments into the ledger via ripgrep (`rg` must be installed — respects `.gitignore`; `project/`, `docs/`, `target/` are excluded).

**Usage:** `myco absorb comments [OPTIONS]`

###### **Options:**

* `--path <PATH>` — Directory to scan (default: the repo root).
* `--kinds <KINDS>` — Comment kinds to match — comma-separated or repeatable (default TODO,FIXME,HACK).
* `--to <TO>` — Where findings land: `backlog` items or `issues` files (default `backlog`).

  Default value: `backlog`
* `--dry` — Report findings without creating items or committing.



## `myco absorb jira`

Import a Jira CSV export. `Done`/`Resolved` rows are skipped unless `--include-done` (created done + archived).

**Usage:** `myco absorb jira [OPTIONS] <file>`

###### **Arguments:**

* `<FILE>` — The export file (`.csv`).

###### **Options:**

* `--include-done` — Import done/resolved issues too.



## `myco absorb github`

Import a GitHub issues dump — a JSON array (`number`, `title`, `state`, `labels`, `assignee`, `body`) or the CSV equivalent. `closed` rows are skipped unless `--include-done` (created done + archived).

**Usage:** `myco absorb github [OPTIONS] <file>`

###### **Arguments:**

* `<FILE>` — The export file (`.json` or `.csv`).

###### **Options:**

* `--include-done` — Import closed issues too.



## `myco index`

The derived index: register repos, query/rebuild snapshots.

**Usage:** `myco index <COMMAND>`

###### **Subcommands:**

* `add` — Register a repo — `<path>` is a repo root (or its `project/` worktree). Ledger-less repos register as `ledger = "none"` with no snapshot; `--org` marks the org/global ledger.
* `remove` — Unregister a repo and drop its snapshot.
* `list` — List registered repos with snapshot freshness.
* `rebuild` — Re-scan a registered repo (all of them without `<path>`).



## `myco index add`

Register a repo — `<path>` is a repo root (or its `project/` worktree). Ledger-less repos register as `ledger = "none"` with no snapshot; `--org` marks the org/global ledger.

**Usage:** `myco index add [OPTIONS] <path>`

###### **Arguments:**

* `<PATH>` — Repo root (or project worktree) to register.

###### **Options:**

* `--org` — Designate this repo as the org/global ledger — the fallback target verbs in ledger-less registered repos resolve to (at most one; clears the mark elsewhere).



## `myco index remove`

Unregister a repo and drop its snapshot.

**Usage:** `myco index remove <path>`

###### **Arguments:**

* `<PATH>` — Registered repo root (or project worktree).



## `myco index list`

List registered repos with snapshot freshness.

**Usage:** `myco index list`



## `myco index rebuild`

Re-scan a registered repo (all of them without `<path>`).

**Usage:** `myco index rebuild [path]`

###### **Arguments:**

* `<PATH>` — Registered repo root (or project worktree).



## `myco org`

Cross-repo scaffolding — re-run `init` on every registered repo. Cross-repo *views* are `myco view org-board|org-status` (index-sourced).

**Usage:** `myco org <COMMAND>`

###### **Subcommands:**

* `init` — Re-run `init` on every registered repo — refreshes the worktree scaffold and root files (.gitignore, the AGENTS.md agent-awareness block). The rollout path for new scaffolding.



## `myco org init`

Re-run `init` on every registered repo — refreshes the worktree scaffold and root files (.gitignore, the AGENTS.md agent-awareness block). The rollout path for new scaffolding.

**Usage:** `myco org init`



## `myco manifest`

Capabilities + protocol version — agent self-discovery.

**Usage:** `myco manifest`



## `myco schema`

The file/state spec, machine-readable JSON Schema.

**Usage:** `myco schema`



## `myco init`

Scaffold `project/`: orphan branch + worktree + .gitignore + seed files + the managed AGENTS.md agent-awareness block. Idempotent — existing worktree and AGENTS.md prose are preserved, only the marked block is (re)written.

**Usage:** `myco init`



## `myco check`

Validate the ledger (schemas, state machine, ids, refs) and the worktree it lives in (detached HEAD, wrong branch, unpushed commits); also validates the managed AGENTS.md block on the repo root. Findings carry a severity: `error` exits 1, `warning` is reported and costs nothing (ADR-0002). `--fix` applies safe repairs + commits, including reattaching a detached worktree.

**Usage:** `myco check [OPTIONS]`

###### **Options:**

* `--fix` — Repair drift in place instead of only reporting.



## `myco migrate`

Convert legacy ledgers to v2: `pm/` → `project/`, or normalize files already on the `project` branch.

**Usage:** `myco migrate`



## `myco commit`

Stage + commit worktree changes (conventional message).

**Usage:** `myco commit [OPTIONS]`

###### **Options:**

* `-m`, `--message <MESSAGE>` — Commit message (default: `chore: update project ledger`).



## `myco undo`

Undo the last ledger commit — resets the `project` branch tip one back (`HEAD~1`). One verb = one commit, so the reset is the inverse mutation, file deletions included; the undone commit is journaled for `redo`. Refuses on a dirty worktree (uncommitted `--no-commit` writes), at the root commit, and when the tip is already published on `origin` (unless `--force`). `--list` shows the redoable stack without mutating.

**Usage:** `myco undo [OPTIONS]`

###### **Options:**

* `--force` — Undo even when the tip commit is already published on the remote.
* `--list` — List the redoable commits (newest first) instead of undoing.



## `myco redo`

Redo the last undo — pops the undo journal and restores the commit `myco undo` moved away from (`git reset --hard <target>`). Repeatable while entries remain; the stack self-invalidates when the ledger moves (any commit/pull/manual git op since the undo). Refuses on a dirty worktree.

**Usage:** `myco redo`



## `myco view`

Declarative views — named presets (`[views.<name>]` in config + built-ins `board|open|epic|milestone`, plus index-sourced `org-board|org-status` for cross-repo) or an ad-hoc spec from flags (`--source index` unions the registered repos' snapshots). `myco view board` is the kanban; `format = "repos"` emits one row per repo; bare `myco view` lists views (or renders `default_view` when config names one).

**Usage:** `myco view [OPTIONS] [name]`

###### **Arguments:**

* `<NAME>` — View name — builtin preset or config view.

###### **Options:**

* `--list` — List defined views and exit.
* `--md` — Emit a markdown table (board/repos formats).
* `--all` — Show every row — no cap.
* `--kind <KIND>` — Item kinds — task|issue|proposal|adr|plan (repeatable/comma-separated).
* `--status <STATUS>` — Statuses — open|claimed|in-progress|blocked|done|deferred (repeatable/comma-separated).
* `--pri-min <PRI_MIN>` — Inclusive lower severity bound — low|med|high|crit.
* `--pri-max <PRI_MAX>` — Inclusive upper severity bound.
* `--label <LABEL>` — `#`-less label — item carries any of these (repeatable/comma-separated).
* `--owner <OWNER>` — Owner actor — item's owner is any of these (repeatable/comma-separated).
* `--claimed` — Items with an owner.
* `--unclaimed` — Items with no owner.
* `--epic <EPIC>` — Items in this epic.
* `--milestone <MILESTONE>` — Items in this milestone.
* `--live-only` — Items not superseded by another (ADRs).
* `--superseded` — Items superseded by another (ADRs).
* `--source <SOURCE>` — Item universe — repo (default) reads this worktree; index unions every registered repo's snapshot (org views).
* `--group-by <GROUP_BY>` — Group axis — status|priority|kind|owner|epic|milestone|repo.
* `--sort <SORT>` — Sort keys — `pri,id,-created`; `-` flips a key's default direction.
* `--limit <N>` — Keep the first N items after sorting.
* `--format <FORMAT>` — Renderer — list|board|repos.



## `myco next`

The single highest-priority open item — "what should I work on" (a `view` eval with limit 1: open tasks+issues, `pri,id`; the filter flags merge over that default). `--claim` mutates it to `claimed` + an owner in one commit.

**Usage:** `myco next [OPTIONS]`

###### **Options:**

* `--kind <KIND>` — Item kinds — task|issue|proposal|adr|plan (repeatable/comma-separated).
* `--status <STATUS>` — Statuses — open|claimed|in-progress|blocked|done|deferred (repeatable/comma-separated).
* `--pri-min <PRI_MIN>` — Inclusive lower severity bound — low|med|high|crit.
* `--pri-max <PRI_MAX>` — Inclusive upper severity bound.
* `--label <LABEL>` — `#`-less label — item carries any of these (repeatable/comma-separated).
* `--owner <OWNER>` — Owner actor — item's owner is any of these (repeatable/comma-separated).
* `--claimed` — Items with an owner.
* `--unclaimed` — Items with no owner.
* `--epic <EPIC>` — Items in this epic.
* `--milestone <MILESTONE>` — Items in this milestone.
* `--live-only` — Items not superseded by another (ADRs).
* `--superseded` — Items superseded by another (ADRs).
* `--sort <SORT>` — Sort keys — `pri,id,-created`; `-` flips a key's default direction. Default `pri,id`.
* `--claim` — Claim the emitted item — status `claimed`, owner set to the `--owner` actor (else `cli`); `--owner` names the claimer here and doesn't filter. Mutating: one commit.
* `--session <SESSION>` — With `--claim`: tag the claim with a session id — appends a `claimed` handoff entry carrying it (`interrupt --session` scopes teardown to it).



## `myco status`

Cheap digest of the work ledger. `--prompt` = token-sized.

**Usage:** `myco status [OPTIONS]`

###### **Options:**

* `--prompt` — Render for an agent prompt (minimal tokens).



## `myco report`

Aging + velocity + blocked — how the ledger is trending.

**Usage:** `myco report`



## `myco handoff`

Attach resumable state at session end — appends a dated entry to `handoffs/<ID>.md` (any item kind, live or archived; append-only).

**Usage:** `myco handoff [OPTIONS] --note <TEXT|-> <id>`

###### **Arguments:**

* `<ID>` — Any item id (`T-NNN`, `I-NNN`, `P-NNN`, `ADR-NNNN`, `PL-NNN`).

###### **Options:**

* `--note <TEXT|->` — Handoff note — done/next/open. `-` reads stdin.
* `--actor <ACTOR>` — Who wrote it (`devin`, `human:luis`, …; default `cli`).
* `--session <SESSION>` — Session provenance — a sepia session id, `cli:<pid>`, any id string.



## `myco interrupt`

A session ended with work still claimed/in-progress/blocked — revert the item to `open` + `interrupted` handoff, one commit. Without an id, `--session <id>` is the teardown: every item claimed under that session reverts in one commit. Session-lifecycle hooks (sepia, CLI session-end) call this; myco never detects sessions itself.

**Usage:** `myco interrupt [OPTIONS] [id]`

###### **Arguments:**

* `<ID>` — Any item id (`T-NNN`, `I-NNN`, `P-NNN`, `ADR-NNNN`, `PL-NNN`) — omit for the `--session` bulk teardown.

###### **Options:**

* `--note <TEXT>` — Extra context appended to the `interrupted` note.
* `--actor <ACTOR>` — Who interrupted it (default `cli`).
* `--session <SESSION>` — Session scope — a sepia session id, `cli:<pid>`, any id string. With `id`: interrupt only when the item's claim tag matches. Without `id`: teardown — interrupt every item claimed under it.



## `myco resume`

Minimal context block to restart work on an item — the item, `blocks:` dep titles, latest handoffs, open checklist. The single-item `status --prompt`. 1..n ids batch in one call.

**Usage:** `myco resume [OPTIONS] <id>...`

###### **Arguments:**

* `<ID>` — Any item id (`T-NNN`, `I-NNN`, `P-NNN`, `ADR-NNNN`, `PL-NNN`) — live or archived.

###### **Options:**

* `--last <LAST>` — Only the latest N handoff entries (default 10).



## `myco ui`

Interactive board TUI (ratatui) — TODO/BACKLOG tabs, item preview, writes through the same ops the verbs use; auto-reloads when the ledger changes on disk. `--view` with a `source = "index"` spec (org-*) opens the cross-repo index board: `repo:ID` rows, verbs write the owning repo.

**Usage:** `myco ui [OPTIONS]`

###### **Options:**

* `--view <NAME>` — Open with a named view's filter narrowing every board; index-sourced views load the cross-repo universe.



## `myco serve`

Sequential request/response verb surface on stdio — JSON-RPC 2.0, one line in / one line out. For embedding (peregrin spawns one per repo); stdin IS the wire, so no served method reads content from it.

**Usage:** `myco serve`



## `myco batch`

One-shot `serve` — NDJSON verb requests from stdin or `--file`, one JSON-RPC response line per request, dispatched through the same served surface. Per-line errors never fail the run; mutations keep per-request commits (`--no-commit`/`$MYCO_NO_COMMIT` apply). stdin IS the wire here too — requests that would read it are rejected.

**Usage:** `myco batch [OPTIONS]`

###### **Options:**

* `--file <PATH>` — Read NDJSON requests from this file instead of stdin.



<hr/>

<small><i>
    This document was generated automatically by
    <a href="https://crates.io/crates/clap-markdown"><code>clap-markdown</code></a>.
</i></small>

