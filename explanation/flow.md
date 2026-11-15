---
url: /myco/explanation/flow.md
---
# Flow: a task's life in the ledger

One end-to-end narrative — what actually happens on disk and in git
when a piece of work goes from "someone said so" to "done". Every hop
is a verb; nothing hand-edits `project/`.

## 1. Capture

A human says "we should ship the thing." The agent offers to record
it; on a yes:

```sh
myco todo add "ship the thing" --pri high --label release
# → added T-001 open pri:high #release ship the thing
```

Under the hood: `meta.toml`'s `[next] t` counter allocates `T-001`,
one line lands in `project/TODO.md`, and the whole thing commits as
`chore: add T-001` on the orphan `project` branch. Your code branch
never sees it.

## 2. Orient

Next session, anyone — human or agent — asks "what's on the board?":

```sh
myco status --prompt
```

The index (redb snapshot, rebuilt lazily if stale) answers without
re-parsing markdown: counts by kind, what's claimed, what's open —
sized for a context window. `myco view board` gives the five-lane
kanban; `myco ui` gives the interactive version.

## 3. Claim and work

```sh
myco item set T-001 --status claimed --owner devin
myco item set T-001 --status in-progress
```

Each is one legal state-machine step and one commit. `owner:devin`
is a namespaced actor string — several agents can share one ledger
without stepping on each other. Deps are data too: `--blocks I-003`
records that this task blocks an issue; `resume` surfaces them.

## 4. Hand off

The session ends mid-work:

```sh
myco handoff T-001 --actor devin --session cli:abc123 \
  --note "Done: draft. Next: screenshots. Open: hero copy."
```

An append-only `##` entry lands in `handoffs/T-001.md`, one commit.
Nothing is lost; nothing is rewritten.

## 5. Resume

A week later, a fresh session:

```sh
myco resume T-001
```

Prints the item, its `blocks:` titles, the latest handoff entries,
and the open `- [ ]` checklist — the minimal restart context. If the
session had *died* rather than ended, `myco interrupt --session
cli:abc123` would have walked claimed items back to `open` with an
`interrupted` marker instead.

## 6. Done

```sh
myco todo done T-001
myco check
```

The line (subtree included) moves from `TODO.md` to `DONE.md` with a
write-once `done:` stamp — completed work stays browsable via
`myco done list` instead of vanishing. `check` validates file spec,
state machine, and index drift; every step above was already a commit
on `project`, so `git -C project log` replays the item's whole life.

## The shape underneath

```
intent ──► markdown (canonical) ──► index (derived) ──► views (reads)
                │                        ▲
                └── verbs ──► commit-per-mutation on `project` branch
```

Capture is a write; orient/resume are reads over the disposable
index; the audit trail is git itself. See
[architecture](architecture.md) for the crates and
[the file spec](file-spec.md) for the grammar each step produces.
