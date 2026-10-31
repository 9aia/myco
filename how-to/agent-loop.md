# Run the daily agent loop

The core loop for an agent (or a human driving like one):
orient → claim → work → hand off → resume. Every step is a verb;
`--json` on all of them.

## Orient

```sh
myco status --prompt    # token-sized digest, sized for a context window
```

`status --prompt` is the session opener: counts by kind, what's
in progress, what's open — enough to pick work without reading
the files. `myco status` (no flag) is the human-readable version;
`myco board` and `myco report` give the kanban and the trend.
Boards cap at a screenful of rows — `--all` shows every card.
For a human at the wheel, `myco ui` is the same board as a
full-screen TUI — see
[the interactive board](../getting-started.md#the-interactive-board).

## Claim

```sh
myco item set T-012 --status claimed --owner devin
myco item set T-012 --status in-progress
```

`claimed` says "an actor owns this" before work starts;
`in-progress` says "being worked now". Both are single legal
steps from `open`. Owner ids are namespaced actor strings —
`devin`, `human:luis`, … — so multiple agents can share a ledger.

Hit a wall mid-task: `myco item set T-012 --status blocked`.
Need the details: `myco item show T-012` (add `--json` for the
full item + location + body).

## Hand off

Before the session ends, leave resumable state on the item:

```sh
myco handoff T-012 --actor devin --session cli:$$ \
  --note "Done: parser landed. Next: emitter. Open: archive semantics."
```

`--note -` reads stdin — pipe a generated summary. See
[handoffs](handoff.md) for the full verb behavior.

## Resume

Next session — human or agent — pulls the restart context:

```sh
myco resume T-012          # compact prompt-ready block
myco resume T-012 --json   # {item, location, blocks_titles, handoffs, open_checklist}
```

Then `myco status --prompt` for the ledger-wide picture and the
loop restarts.

## Hygiene

```sh
myco check          # exits 1 with findings
myco check --fix    # safe repairs + one commit
```

Run `check` when files were hand-edited or the index may have
drifted — it validates the [file spec](../explanation/file-spec.md),
state machine, and index snapshots.

## No-commit mode

Batch or scripted flows can skip the per-verb commit with
`--no-commit` or `MYCO_NO_COMMIT=1`, then commit once:

```sh
myco commit -m "chore: end-of-day ledger tidy"
```

`myco commit` stages all worktree changes and commits with a
conventional message (default `chore: update project ledger`).
