# Command-Line Help for `myco`

This document contains the help content for the `myco` command-line program.

**Command Overview:**

* [`myco`↴](#myco)
* [`myco manifest`↴](#myco-manifest)
* [`myco schema`↴](#myco-schema)
* [`myco init`↴](#myco-init)
* [`myco check`↴](#myco-check)
* [`myco migrate`↴](#myco-migrate)
* [`myco commit`↴](#myco-commit)
* [`myco todo`↴](#myco-todo)
* [`myco todo add`↴](#myco-todo-add)
* [`myco todo done`↴](#myco-todo-done)
* [`myco todo defer`↴](#myco-todo-defer)
* [`myco todo list`↴](#myco-todo-list)
* [`myco backlog`↴](#myco-backlog)
* [`myco backlog add`↴](#myco-backlog-add)
* [`myco backlog promote`↴](#myco-backlog-promote)
* [`myco backlog list`↴](#myco-backlog-list)
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
* [`myco handoff`↴](#myco-handoff)
* [`myco resume`↴](#myco-resume)
* [`myco status`↴](#myco-status)
* [`myco board`↴](#myco-board)
* [`myco epic`↴](#myco-epic)
* [`myco milestone`↴](#myco-milestone)
* [`myco absorb`↴](#myco-absorb)
* [`myco absorb comments`↴](#myco-absorb-comments)
* [`myco absorb jira`↴](#myco-absorb-jira)
* [`myco absorb github`↴](#myco-absorb-github)
* [`myco report`↴](#myco-report)
* [`myco index`↴](#myco-index)
* [`myco index add`↴](#myco-index-add)
* [`myco index remove`↴](#myco-index-remove)
* [`myco index list`↴](#myco-index-list)
* [`myco index rebuild`↴](#myco-index-rebuild)
* [`myco org`↴](#myco-org)
* [`myco org status`↴](#myco-org-status)
* [`myco org board`↴](#myco-org-board)
* [`myco org init`↴](#myco-org-init)

## `myco`

myco — project management, grown not built: agent-first PM CLI

**Usage:** `myco [OPTIONS] <COMMAND>`

###### **Subcommands:**

* `manifest` — Capabilities + protocol version — agent self-discovery
* `schema` — The file/state spec, machine-readable JSON Schema
* `init` — Scaffold `project/`: orphan branch + worktree + .gitignore + seed files + the managed AGENTS.md agent-awareness block. Idempotent — existing worktree and AGENTS.md prose are preserved, only the marked block is (re)written
* `check` — Validate schemas + state machine, repair file/index drift; also validates the managed AGENTS.md block on the repo root. Exit 1 with findings; `--fix` applies safe repairs + commits
* `migrate` — Convert legacy ledgers to v2: `pm/` → `project/`, or normalize files already on the `project` branch
* `commit` — Stage + commit worktree changes (conventional message)
* `todo` — TODO.md tasks — add / done / defer / list
* `backlog` — BACKLOG.md parked items — add / promote / list
* `issue` — Tracked issues, one file each under `issues/`
* `proposal` — Design proposals under `proposals/` (`P-NNN`)
* `plan` — Execution plans under `plans/` (`PL-NNN`)
* `adr` — Architecture decision records under `adrs/` (`ADR-NNNN`)
* `item` — Kind-agnostic item verbs — show / set
* `handoff` — Attach resumable state at session end — appends a dated entry to `handoffs/<ID>.md` (any item kind, live or archived; append-only)
* `resume` — Minimal context block to restart work on an item — the item, `blocks:` dep titles, latest handoffs, open checklist. The single-item `status --prompt`
* `status` — Cheap digest of the work ledger. `--prompt` = token-sized
* `board` — Kanban of the ledger — Backlog | Open | Claimed | In Progress | Blocked. `--md` emits a markdown table
* `epic` — Items grouped by epic — bare lists epics with counts, a name renders that epic's board (`epic v2` ≡ `board --epic v2`)
* `milestone` — Items grouped by milestone — bare lists milestones with counts, a name renders that milestone's board
* `absorb` — Import work into the ledger — TODO/FIXME comments, Jira CSV exports, GitHub issue dumps
* `report` — Aging + velocity + blocked — how the ledger is trending
* `index` — The derived index: register repos, query/rebuild snapshots
* `org` — Cross-repo views over the index (auto-rebuilds stale repos)

###### **Options:**

* `--json` — Emit JSON (accepted everywhere; default output for machines)
* `--no-commit` — Skip the per-verb commit (also suppressed by $MYCO_NO_COMMIT)
* `--dir <PATH>` — Project worktree to act on, overriding discovery
* `--root <PATH>` — Repo root to act on, overriding discovery (the worktree is `<root>/project`)



## `myco manifest`

Capabilities + protocol version — agent self-discovery

**Usage:** `myco manifest`



## `myco schema`

The file/state spec, machine-readable JSON Schema

**Usage:** `myco schema`



## `myco init`

Scaffold `project/`: orphan branch + worktree + .gitignore + seed files + the managed AGENTS.md agent-awareness block. Idempotent — existing worktree and AGENTS.md prose are preserved, only the marked block is (re)written

**Usage:** `myco init`



## `myco check`

Validate schemas + state machine, repair file/index drift; also validates the managed AGENTS.md block on the repo root. Exit 1 with findings; `--fix` applies safe repairs + commits

**Usage:** `myco check [OPTIONS]`

###### **Options:**

* `--fix` — Repair drift in place instead of only reporting



## `myco migrate`

Convert legacy ledgers to v2: `pm/` → `project/`, or normalize files already on the `project` branch

**Usage:** `myco migrate`



## `myco commit`

Stage + commit worktree changes (conventional message)

**Usage:** `myco commit [OPTIONS]`

###### **Options:**

* `-m`, `--message <MESSAGE>` — Commit message (default: `chore: update project ledger`)



## `myco todo`

TODO.md tasks — add / done / defer / list

**Usage:** `myco todo <COMMAND>`

###### **Subcommands:**

* `add` — Add a task — allocs a T-id, appends to a section of TODO.md
* `done` — Mark done — walks the state machine to `done`, then removes the line (git history keeps the record). Done on a parent removes the whole subtree — children die with it
* `defer` — Park a task — transition to `deferred`, move the line to BACKLOG.md. Defer on a parent moves the whole subtree (children park with it); on a child it detaches alone
* `list` — List tasks



## `myco todo add`

Add a task — allocs a T-id, appends to a section of TODO.md

**Usage:** `myco todo add [OPTIONS] <TITLE>`

###### **Arguments:**

* `<TITLE>` — One-line title

###### **Options:**

* `--section <SECTION>` — Section to add under (created when absent; default: the first section, or an unsectioned run)
* `--parent <PARENT>` — Nest under this task instead of a section — the new item becomes a child (`  - [ ]` under the parent)
* `--pri <PRI>` [alias: `priority`] — Priority: low|med|high|crit
* `--owner <OWNER>` — Owner actor id (`devin`, `human:luis`, …)
* `--label <LABEL>` — `#label` — repeatable
* `--blocks <BLOCKS>` — Ids this item blocks — comma-separated or repeatable
* `--epic <EPIC>` — Epic name — the delivery grouping this item belongs to (`[A-Za-z0-9_.-]+`)
* `--milestone <MILESTONE>` — Milestone name — the target this item ships under (`[A-Za-z0-9_.-]+`)



## `myco todo done`

Mark done — walks the state machine to `done`, then removes the line (git history keeps the record). Done on a parent removes the whole subtree — children die with it

**Usage:** `myco todo done <ID>`

###### **Arguments:**

* `<ID>` — Task id (`T-NNN`)



## `myco todo defer`

Park a task — transition to `deferred`, move the line to BACKLOG.md. Defer on a parent moves the whole subtree (children park with it); on a child it detaches alone

**Usage:** `myco todo defer <ID>`

###### **Arguments:**

* `<ID>` — Task id (`T-NNN`)



## `myco todo list`

List tasks

**Usage:** `myco todo list [OPTIONS]`

###### **Options:**

* `--status <STATUS>` — Keep only items with this status
* `--owner <OWNER>` — Keep only items owned by this actor
* `--label <LABEL>` — Keep only items carrying all of these labels (repeatable)
* `--limit <N>` — Keep only the first N items after filtering



## `myco backlog`

BACKLOG.md parked items — add / promote / list

**Usage:** `myco backlog <COMMAND>`

###### **Subcommands:**

* `add` — Add a deferred item — allocs a T-id straight into BACKLOG.md
* `promote` — Re-activate a parked item — `deferred → open`, move the line to TODO.md
* `list` — List backlog items



## `myco backlog add`

Add a deferred item — allocs a T-id straight into BACKLOG.md

**Usage:** `myco backlog add [OPTIONS] <TITLE>`

###### **Arguments:**

* `<TITLE>` — One-line title

###### **Options:**

* `--section <SECTION>` — Section to add under (created when absent; default: the first section, or an unsectioned run)
* `--pri <PRI>` [alias: `priority`] — Priority: low|med|high|crit
* `--owner <OWNER>` — Owner actor id (`devin`, `human:luis`, …)
* `--label <LABEL>` — `#label` — repeatable
* `--blocks <BLOCKS>` — Ids this item blocks — comma-separated or repeatable
* `--epic <EPIC>` — Epic name — the delivery grouping this item belongs to (`[A-Za-z0-9_.-]+`)
* `--milestone <MILESTONE>` — Milestone name — the target this item ships under (`[A-Za-z0-9_.-]+`)



## `myco backlog promote`

Re-activate a parked item — `deferred → open`, move the line to TODO.md

**Usage:** `myco backlog promote <ID>`

###### **Arguments:**

* `<ID>` — Task id (`T-NNN`)



## `myco backlog list`

List backlog items

**Usage:** `myco backlog list [OPTIONS]`

###### **Options:**

* `--status <STATUS>` — Keep only items with this status
* `--owner <OWNER>` — Keep only items owned by this actor
* `--label <LABEL>` — Keep only items carrying all of these labels (repeatable)
* `--limit <N>` — Keep only the first N items after filtering



## `myco issue`

Tracked issues, one file each under `issues/`

**Usage:** `myco issue <COMMAND>`

###### **Subcommands:**

* `new` — File an issue — allocs an I-id, writes `issues/issue-NNNN-<slug>.md` with frontmatter + template
* `list` — List issues (live files; `--archived` includes `issues/archive/`)
* `resolve` — Close an issue — walks the state machine to `done`; `--resolution` appends text under `## Resolution`
* `archive` — Shelve a done issue — moves the file to `issues/archive/`
* `comment` — Append a dated note under `## Comments` (created when missing)



## `myco issue new`

File an issue — allocs an I-id, writes `issues/issue-NNNN-<slug>.md` with frontmatter + template

**Usage:** `myco issue new [OPTIONS] <TITLE>`

###### **Arguments:**

* `<TITLE>` — One-line title (drives the filename slug)

###### **Options:**

* `--body <FILE|->` — Body markdown from a file, or `-` for stdin
* `--text <TEXT>` — Body markdown inline
* `--pri <PRI>` [alias: `priority`] — Priority: low|med|high|crit
* `--owner <OWNER>` — Owner actor id (`devin`, `human:luis`, …)
* `--label <LABEL>` — `#label` — repeatable
* `--blocks <BLOCKS>` — Ids this item blocks — comma-separated or repeatable
* `--epic <EPIC>` — Epic name — the delivery grouping this item belongs to (`[A-Za-z0-9_.-]+`)
* `--milestone <MILESTONE>` — Milestone name — the target this item ships under (`[A-Za-z0-9_.-]+`)



## `myco issue list`

List issues (live files; `--archived` includes `issues/archive/`)

**Usage:** `myco issue list [OPTIONS]`

###### **Options:**

* `--archived` — Include archived issues
* `--status <STATUS>` — Keep only items with this status
* `--owner <OWNER>` — Keep only items owned by this actor
* `--label <LABEL>` — Keep only items carrying all of these labels (repeatable)
* `--limit <N>` — Keep only the first N items after filtering



## `myco issue resolve`

Close an issue — walks the state machine to `done`; `--resolution` appends text under `## Resolution`

**Usage:** `myco issue resolve [OPTIONS] <ID>`

###### **Arguments:**

* `<ID>` — Issue id (`I-NNN`)

###### **Options:**

* `--resolution <RESOLUTION>` — Resolution text appended to the body



## `myco issue archive`

Shelve a done issue — moves the file to `issues/archive/`

**Usage:** `myco issue archive <ID>`

###### **Arguments:**

* `<ID>` — Issue id (`I-NNN`)



## `myco issue comment`

Append a dated note under `## Comments` (created when missing)

**Usage:** `myco issue comment <ID> <TEXT>`

###### **Arguments:**

* `<ID>` — Issue id (`I-NNN`)
* `<TEXT>` — Comment text



## `myco proposal`

Design proposals under `proposals/` (`P-NNN`)

**Usage:** `myco proposal <COMMAND>`

###### **Subcommands:**

* `new` — File a proposal — allocs a P-id, writes the proposal file with frontmatter + template
* `list` — List proposals
* `status` — Set a proposal's status. Proposal words alias the state machine: `draft` = open, `accepted` = done, `rejected` = deferred — the transition is walked through the machine, so `accepted` passes `open → in-progress → done`



## `myco proposal new`

File a proposal — allocs a P-id, writes the proposal file with frontmatter + template

**Usage:** `myco proposal new [OPTIONS] <TITLE>`

###### **Arguments:**

* `<TITLE>` — One-line title (drives the filename slug)

###### **Options:**

* `--body <FILE|->` — Body markdown from a file, or `-` for stdin
* `--text <TEXT>` — Body markdown inline
* `--pri <PRI>` [alias: `priority`] — Priority: low|med|high|crit
* `--owner <OWNER>` — Owner actor id (`devin`, `human:luis`, …)
* `--label <LABEL>` — `#label` — repeatable
* `--blocks <BLOCKS>` — Ids this item blocks — comma-separated or repeatable
* `--epic <EPIC>` — Epic name — the delivery grouping this item belongs to (`[A-Za-z0-9_.-]+`)
* `--milestone <MILESTONE>` — Milestone name — the target this item ships under (`[A-Za-z0-9_.-]+`)



## `myco proposal list`

List proposals

**Usage:** `myco proposal list [OPTIONS]`

###### **Options:**

* `--status <STATUS>` — Keep only items with this status
* `--owner <OWNER>` — Keep only items owned by this actor
* `--label <LABEL>` — Keep only items carrying all of these labels (repeatable)
* `--limit <N>` — Keep only the first N items after filtering



## `myco proposal status`

Set a proposal's status. Proposal words alias the state machine: `draft` = open, `accepted` = done, `rejected` = deferred — the transition is walked through the machine, so `accepted` passes `open → in-progress → done`

**Usage:** `myco proposal status <ID> <STATUS>`

###### **Arguments:**

* `<ID>` — Proposal id (`P-NNN`)
* `<STATUS>` — `draft` | `accepted` | `rejected` (raw machine statuses like `in-progress` also accepted)



## `myco plan`

Execution plans under `plans/` (`PL-NNN`)

**Usage:** `myco plan <COMMAND>`

###### **Subcommands:**

* `new` — File a plan — allocs a PL-id, writes the plan file with frontmatter + a phase-checklist template
* `list` — List plans



## `myco plan new`

File a plan — allocs a PL-id, writes the plan file with frontmatter + a phase-checklist template

**Usage:** `myco plan new [OPTIONS] <TITLE>`

###### **Arguments:**

* `<TITLE>` — One-line title (drives the filename slug)

###### **Options:**

* `--body <FILE|->` — Body markdown from a file, or `-` for stdin
* `--text <TEXT>` — Body markdown inline
* `--pri <PRI>` [alias: `priority`] — Priority: low|med|high|crit
* `--owner <OWNER>` — Owner actor id (`devin`, `human:luis`, …)
* `--label <LABEL>` — `#label` — repeatable
* `--blocks <BLOCKS>` — Ids this item blocks — comma-separated or repeatable
* `--epic <EPIC>` — Epic name — the delivery grouping this item belongs to (`[A-Za-z0-9_.-]+`)
* `--milestone <MILESTONE>` — Milestone name — the target this item ships under (`[A-Za-z0-9_.-]+`)



## `myco plan list`

List plans

**Usage:** `myco plan list [OPTIONS]`

###### **Options:**

* `--status <STATUS>` — Keep only items with this status
* `--owner <OWNER>` — Keep only items owned by this actor
* `--label <LABEL>` — Keep only items carrying all of these labels (repeatable)
* `--limit <N>` — Keep only the first N items after filtering



## `myco adr`

Architecture decision records under `adrs/` (`ADR-NNNN`)

**Usage:** `myco adr <COMMAND>`

###### **Subcommands:**

* `new` — File an ADR — allocs an ADR-id, writes the file with frontmatter (status `open` = "proposed") + template
* `list` — List ADRs
* `supersede` — Mark an ADR superseded by another — the old file gets `superseded_by:` + status `done`, the replacement gets a reciprocal `supersedes:` key. Both must already exist



## `myco adr new`

File an ADR — allocs an ADR-id, writes the file with frontmatter (status `open` = "proposed") + template

**Usage:** `myco adr new [OPTIONS] <TITLE>`

###### **Arguments:**

* `<TITLE>` — One-line title (drives the filename slug)

###### **Options:**

* `--body <FILE|->` — Body markdown from a file, or `-` for stdin
* `--text <TEXT>` — Body markdown inline
* `--pri <PRI>` [alias: `priority`] — Priority: low|med|high|crit
* `--owner <OWNER>` — Owner actor id (`devin`, `human:luis`, …)
* `--label <LABEL>` — `#label` — repeatable
* `--blocks <BLOCKS>` — Ids this item blocks — comma-separated or repeatable
* `--epic <EPIC>` — Epic name — the delivery grouping this item belongs to (`[A-Za-z0-9_.-]+`)
* `--milestone <MILESTONE>` — Milestone name — the target this item ships under (`[A-Za-z0-9_.-]+`)



## `myco adr list`

List ADRs

**Usage:** `myco adr list [OPTIONS]`

###### **Options:**

* `--status <STATUS>` — Keep only items with this status
* `--owner <OWNER>` — Keep only items owned by this actor
* `--label <LABEL>` — Keep only items carrying all of these labels (repeatable)
* `--limit <N>` — Keep only the first N items after filtering



## `myco adr supersede`

Mark an ADR superseded by another — the old file gets `superseded_by:` + status `done`, the replacement gets a reciprocal `supersedes:` key. Both must already exist

**Usage:** `myco adr supersede --by <BY> <ID>`

###### **Arguments:**

* `<ID>` — The ADR being replaced (`ADR-NNNN`)

###### **Options:**

* `--by <BY>` — The ADR replacing it (`ADR-NNNN`)



## `myco item`

Kind-agnostic item verbs — show / set

**Usage:** `myco item <COMMAND>`

###### **Subcommands:**

* `show` — Dump one item — frontmatter fields plus the body for file-backed kinds (`--json` = the full Item + location + body)
* `set` — Mutate an item in place. `--status` is the only field routed through the state machine — single steps only, invalid ones error with the valid targets. Status changes on tasks move the line between TODO.md and BACKLOG.md (`done` deletes it)



## `myco item show`

Dump one item — frontmatter fields plus the body for file-backed kinds (`--json` = the full Item + location + body)

**Usage:** `myco item show <ID>`

###### **Arguments:**

* `<ID>` — Any item id (`T-NNN`, `I-NNN`, `P-NNN`, `ADR-NNNN`, `PL-NNN`) — live or archived



## `myco item set`

Mutate an item in place. `--status` is the only field routed through the state machine — single steps only, invalid ones error with the valid targets. Status changes on tasks move the line between TODO.md and BACKLOG.md (`done` deletes it)

**Usage:** `myco item set [OPTIONS] <ID>`

###### **Arguments:**

* `<ID>` — Any item id

###### **Options:**

* `--status <STATUS>` — New status — one legal step from the current one
* `--pri <PRI>` [alias: `priority`] — Priority: low|med|high|crit
* `--owner <OWNER>` — Owner actor id
* `--label <LABEL>` — Add (`+x` or `x`) or remove (`-x`) a label — repeatable
* `--unlabel <UNLABEL>` — Remove a label — repeatable
* `--blocks <BLOCKS>` — Add (`+T-1` or `T-1`) or remove (`-T-1`) a blocks dep — repeatable
* `--unblock <UNBLOCK>` — Remove a blocks dep — repeatable
* `--epic <EPIC>` — Epic name — `""` clears the epic
* `--milestone <MILESTONE>` — Milestone name — `""` clears the milestone



## `myco handoff`

Attach resumable state at session end — appends a dated entry to `handoffs/<ID>.md` (any item kind, live or archived; append-only)

**Usage:** `myco handoff [OPTIONS] --note <TEXT|-> <ID>`

###### **Arguments:**

* `<ID>` — Any item id (`T-NNN`, `I-NNN`, `P-NNN`, `ADR-NNNN`, `PL-NNN`)

###### **Options:**

* `--note <TEXT|->` — Handoff note — done/next/open. `-` reads stdin
* `--actor <ACTOR>` — Who wrote it (`devin`, `human:luis`, …; default `cli`)
* `--session <SESSION>` — Session provenance — a sepia session id, `cli:<pid>`, any id string



## `myco resume`

Minimal context block to restart work on an item — the item, `blocks:` dep titles, latest handoffs, open checklist. The single-item `status --prompt`

**Usage:** `myco resume [OPTIONS] <ID>`

###### **Arguments:**

* `<ID>` — Any item id

###### **Options:**

* `--last <LAST>` — Only the latest N handoff entries (default 10)



## `myco status`

Cheap digest of the work ledger. `--prompt` = token-sized

**Usage:** `myco status [OPTIONS]`

###### **Options:**

* `--prompt` — Render for an agent prompt (minimal tokens)



## `myco board`

Kanban of the ledger — Backlog | Open | Claimed | In Progress | Blocked. `--md` emits a markdown table

**Usage:** `myco board [OPTIONS]`

###### **Options:**

* `--md` — Emit a markdown table (for docs/READMEs)
* `--all` — Show every card — no row cap
* `--epic <EPIC>` — Only items in this epic
* `--milestone <MILESTONE>` — Only items in this milestone



## `myco epic`

Items grouped by epic — bare lists epics with counts, a name renders that epic's board (`epic v2` ≡ `board --epic v2`)

**Usage:** `myco epic [OPTIONS] [NAME]`

###### **Arguments:**

* `<NAME>` — Epic name — omit for the all-epics summary

###### **Options:**

* `--md` — Emit a markdown table (with a name)
* `--all` — Show every card — no row cap (with a name)



## `myco milestone`

Items grouped by milestone — bare lists milestones with counts, a name renders that milestone's board

**Usage:** `myco milestone [OPTIONS] [NAME]`

###### **Arguments:**

* `<NAME>` — Milestone name — omit for the all-milestones summary

###### **Options:**

* `--md` — Emit a markdown table (with a name)
* `--all` — Show every card — no row cap (with a name)



## `myco absorb`

Import work into the ledger — TODO/FIXME comments, Jira CSV exports, GitHub issue dumps

**Usage:** `myco absorb <COMMAND>`

###### **Subcommands:**

* `comments` — Sweep code comments into the ledger via ripgrep (`rg` must be installed — respects `.gitignore`; `project/`, `docs/`, `target/` are excluded)
* `jira` — Import a Jira CSV export. `Done`/`Resolved` rows are skipped unless `--include-done` (created done + archived)
* `github` — Import a GitHub issues dump — a JSON array (`number`, `title`, `state`, `labels`, `assignee`, `body`) or the CSV equivalent. `closed` rows are skipped unless `--include-done` (created done + archived)



## `myco absorb comments`

Sweep code comments into the ledger via ripgrep (`rg` must be installed — respects `.gitignore`; `project/`, `docs/`, `target/` are excluded)

**Usage:** `myco absorb comments [OPTIONS]`

###### **Options:**

* `--path <PATH>` — Directory to scan (default: the repo root)
* `--kinds <KINDS>` — Comment kinds to match — comma-separated or repeatable (default TODO,FIXME,HACK)
* `--to <TO>` — Where findings land: `backlog` items or `issues` files (default `backlog`)

  Default value: `backlog`
* `--dry` — Report findings without creating items or committing



## `myco absorb jira`

Import a Jira CSV export. `Done`/`Resolved` rows are skipped unless `--include-done` (created done + archived)

**Usage:** `myco absorb jira [OPTIONS] <FILE>`

###### **Arguments:**

* `<FILE>` — The export file (`.csv`)

###### **Options:**

* `--include-done` — Import done/resolved issues too



## `myco absorb github`

Import a GitHub issues dump — a JSON array (`number`, `title`, `state`, `labels`, `assignee`, `body`) or the CSV equivalent. `closed` rows are skipped unless `--include-done` (created done + archived)

**Usage:** `myco absorb github [OPTIONS] <FILE>`

###### **Arguments:**

* `<FILE>` — The export file (`.json` or `.csv`)

###### **Options:**

* `--include-done` — Import closed issues too



## `myco report`

Aging + velocity + blocked — how the ledger is trending

**Usage:** `myco report`



## `myco index`

The derived index: register repos, query/rebuild snapshots

**Usage:** `myco index <COMMAND>`

###### **Subcommands:**

* `add` — Register a repo — `<path>` is a repo root (or its `project/` worktree). Takes an initial snapshot
* `remove` — Unregister a repo and drop its snapshot
* `list` — List registered repos with snapshot freshness
* `rebuild` — Re-scan a registered repo (all of them without `<path>`)



## `myco index add`

Register a repo — `<path>` is a repo root (or its `project/` worktree). Takes an initial snapshot

**Usage:** `myco index add <PATH>`

###### **Arguments:**

* `<PATH>` — Repo root (or project worktree) to register



## `myco index remove`

Unregister a repo and drop its snapshot

**Usage:** `myco index remove <PATH>`

###### **Arguments:**

* `<PATH>` — Registered repo root (or project worktree)



## `myco index list`

List registered repos with snapshot freshness

**Usage:** `myco index list`



## `myco index rebuild`

Re-scan a registered repo (all of them without `<path>`)

**Usage:** `myco index rebuild [PATH]`

###### **Arguments:**

* `<PATH>` — Registered repo root (or project worktree)



## `myco org`

Cross-repo views over the index (auto-rebuilds stale repos)

**Usage:** `myco org <COMMAND>`

###### **Subcommands:**

* `status` — Per-repo counts + org totals + items grouped by owner
* `board` — Org kanban — repo-prefixed cards over every live item
* `init` — Re-run `init` on every registered repo — refreshes the worktree scaffold and root files (.gitignore, the AGENTS.md agent-awareness block). The rollout path for new scaffolding



## `myco org status`

Per-repo counts + org totals + items grouped by owner

**Usage:** `myco org status [OPTIONS]`

###### **Options:**

* `--all` — Show repos with no live items too



## `myco org board`

Org kanban — repo-prefixed cards over every live item

**Usage:** `myco org board [OPTIONS]`

###### **Options:**

* `--md` — Emit a markdown table (for docs/READMEs)
* `--all` — Show every card — no row cap
* `--repo <REPO>` — Only this repo's items (name or registered path)



## `myco org init`

Re-run `init` on every registered repo — refreshes the worktree scaffold and root files (.gitignore, the AGENTS.md agent-awareness block). The rollout path for new scaffolding

**Usage:** `myco org init`



<hr/>

<small><i>
    This document was generated automatically by
    <a href="https://crates.io/crates/clap-markdown"><code>clap-markdown</code></a>.
</i></small>

