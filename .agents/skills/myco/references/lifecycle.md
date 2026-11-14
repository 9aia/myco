# Lifecycle — init / check / migrate / commit

The `project/` ledger lives on an orphan `project` branch, mounted as
a git worktree at `<repo>/project` and gitignored on `main`. Nothing
under `project/` is ever committed to the working branch — the
orphan-branch history is the audit trail.

## Auto-commit semantics

Every mutating verb ends in one atomic commit on the `project`
branch (`chore: <verb> <id>`). No commit when the tree didn't change.
Opt out with `--no-commit` or `$MYCO_NO_COMMIT` (tests, dry runs);
batch up later with `myco commit -m "…"`.

Each commit body carries an `Actor: <id>` trailer — the verb's
`--actor`/`--owner` where it takes one, else the resolved actor
(`$MYCO_ACTOR` → `human:<git user.name>` → `cli`).
Author/committer stay the repo's git identity; actor attribution
lives in the trailer.

## `myco init` — scaffold a repo

```bash
myco init            # inside the repo (or --root <path>)
```

Creates the orphan branch + worktree + `meta.toml` + seed files,
appends `project/` to `.gitignore`, and syncs the managed
`<!-- myco:agent-awareness -->` block into `AGENTS.md` (both
committed on the current branch — prose outside the markers is
preserved; a hand-written `## Agent awareness` section is upgraded
in place). Idempotent — a valid existing worktree gets scaffold
gaps repaired, never overwritten. Refuses to clobber a `project/`
dir that isn't a worktree: move it aside first, then init and copy
content back.

`myco org init` re-runs init across every repo registered in the
index — the rollout path for new scaffolding.

## `myco check` — validate + repair

```bash
myco check           # report findings, exit 1 when dirty
myco check --fix     # apply safe repairs, commit
```

Validates the file spec and the state machine, detects file/index
drift — plus the root `AGENTS.md` managed block (`agents-md`
finding; `--fix` writes/appends/rewrites it and commits on the
current branch). Run it after any forced hand-edit, after
merge/rebase surgery inside the worktree, and as the CI gate.

## `myco migrate` — legacy → v2

Auto-detects two modes:

- `pm/` dir at the repo root → converted into `project/` (inits the
  worktree first when missing), `pm/` deletion committed on main.
- Files already on the `project` branch → normalized in place:
  id-less `- [ ]` bullets get ids, `[x]`/done lines drop (history keeps
  them), heading-like lines become sections, unfrontmattered files in
  `issues/`/`proposals/`/`plans/`/`adrs/` are adopted (fresh id +
  frontmatter + canonical filename, body kept verbatim), `index.md`
  links are repointed, everything re-emitted canonically.

Always read the JSON report (`--json`) and spot-check
`git -C project log`/`diff` for content loss afterward.

## `myco sync` — reconcile the ledger branch

Mutating verbs refuse while the `project` branch is behind
`origin/<branch>` — every verb commit would deepen the divergence.
`myco sync` fetches, fast-forwards or rebases local commits onto
the remote tip, then heals merge damage: `meta.toml` counters
reconcile and duplicate ids renumber **remote-wins** — the pushed
item keeps its id, the local one moves to a fresh id, inbound
`blocks:`/`links:`/supersede refs rebind to it, and a `renumbered
<old> → <new>` handoff entry records the audit.

Before the behind-check, mutating verbs run a best-effort
`git fetch origin <branch>` — `[sync] auto_fetch = false` in
`myco.toml` opts out (a stale remote-tracking ref can only
*under*-report divergence; the fetch is never fatal when it
fails).

`[sync] auto_push` (default on; `false` opts out) publishes:
every committed mutation (and a successful `myco sync`) pushes
the `project` branch to `origin`. A failed push warns on stderr
but never fails the verb; `myco check`'s unpushed-commits
finding is the backstop.

A rebase conflict stops mid-flight: resolve + `git rebase
--continue` + `myco sync` again, or `myco sync --abort` to bail.

## `myco commit` — manual sweep

Stage + commit outstanding worktree changes with a conventional
message (`-m` to override). Use after hand-edits or `--no-commit`
batches — worktree changes are otherwise left dirty.

## Discovery — how verbs find the worktree

`--dir <worktree>` → `--root <repo>` (`<root>/project`) →
`myco.toml`'s `project_dir` → walk up from cwd for a qualifying
`project/` → cwd inside the worktree itself. From anywhere inside a
repo, plain `myco <verb>` just works.

Note: project-mode expects checkbox items — freeform `- ` bullets
(pm-style) are NOT converted and will error; use pm-mode, or import
them via `myco todo add`/`myco backlog add` (or `myco absorb`).
