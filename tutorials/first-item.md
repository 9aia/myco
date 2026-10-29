# Your first item

A guided walk through one task's lifecycle: scaffold a ledger,
create a task, move it through the state machine, and close it —
seeing the markdown change under you each step.

You'll need `myco` installed (see
[getting started](../getting-started.md)) and a git repo to play
in:

```sh
mkdir ledger-demo && cd ledger-demo
git init && git commit --allow-empty -m init
```

## 1. Scaffold the ledger

```sh
myco init
```

This creates a `project/` directory — actually a git worktree of
the new orphan `project` branch — seeded with `TODO.md`,
`BACKLOG.md`, `index.md`, `meta.toml`, and empty item dirs.
`git worktree list` shows it; `.gitignore` hides it from your
code branch.

## 2. Add a task

```sh
myco todo add "write the tutorial" --pri high --label docs
```

```text
added T-001 open pri:high #docs write the tutorial
```

myco allocated `T-001` (the `t` counter in `meta.toml`) and
appended one line to `project/TODO.md`:

```markdown
- [ ] T-001 write the tutorial · pri:high · created:2026-10-29 · #docs
```

Everything after the title is `·`-separated tokens —
`pri:`, `owner:`, `#label`, `blocks:`, `epic:`, `ms:`.

Nest a sub-task under it:

```sh
myco todo add "review it" --parent T-001
```

Children are `- [ ]` lines indented two spaces under the parent.
They are full items — same metadata, same state machine — and
`done`/`defer` on a parent act on the whole subtree.

## 3. Move it through the state machine

```sh
myco item set T-001 --status in-progress --owner devin
myco board
```

```text
+---------+-----------------+---------+--------------------------+---------+
| Backlog | Open            | Claimed | In Progress              | Blocked |
+---------+-----------------+---------+--------------------------+---------+
|         | T-002 review it |         | T-001 write the tutorial |         |
+---------+-----------------+---------+--------------------------+---------+
```

Statuses are `open → claimed → in-progress → done`, with
`blocked` and `deferred` off the main path — see
[the file spec](../explanation/file-spec.md#state-machine) for
the legal transitions. `item set` walks the machine one step at a
time; an illegal jump errors and tells you the valid targets.

Park something instead of finishing it:

```sh
myco todo defer T-002   # → deferred; the line moves to BACKLOG.md
myco backlog promote T-002   # → open; back to TODO.md
```

## 4. Leave a handoff

```sh
myco handoff T-001 --note "Done: draft. Next: screenshots." --actor devin
myco resume T-001
```

`handoff` appends a dated entry to `handoffs/T-001.md`;
`resume` prints the minimal restart context — the item, its
`blocks:` deps, latest handoffs, open checklist.

## 5. Finish and check

```sh
myco todo done T-001
myco check
```

```text
check: clean
```

Done tasks are *removed* from `TODO.md` — the `project` branch
history is the record. Every verb you ran was one atomic commit
on that branch (`git -C project log --oneline` shows them all).

## Recap

| you ran | myco did |
|---|---|
| `todo add` | alloc id, append line to `TODO.md` |
| `item set --status` | one legal state-machine step |
| `todo defer` / `backlog promote` | move the line TODO ↔ BACKLOG |
| `handoff` / `resume` | append-only session notes / restart context |
| `todo done` | `done`, then delete the line |
| `check` | validate files, state machine, index drift |

Every write went through a verb — hand-editing works too (the
markdown is canonical), but `myco check` is the referee either
way.

Next: [the daily agent loop](../how-to/agent-loop.md) for how an
agent drives these verbs, or the
[CLI reference](../reference/cli.md) for the full surface.
