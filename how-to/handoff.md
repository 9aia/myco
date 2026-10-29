# Hand off work between sessions

Attach resumable state to any item at the end of a working
session, then pull the minimal context to restart it later.

```sh
myco handoff T-012 --note "parser done, emitter next" --actor devin --session cli:abc123
myco resume T-012   # minimal context block to restart work
```

## `myco handoff <id>`

Appends a dated entry to `handoffs/<ID>.md` — works on every item
kind (`T-`, `I-`, `P-`, `ADR-`, `PL-`), live or archived. The item
must exist; the note is required.

- `--note "…"` — the handoff text (done/next/open). `-` reads stdin.
- `--actor <id>` — who wrote it (`devin`, `human:luis`; default `cli`).
- `--session <id>` — provenance, any id string (a sepia session id,
  `cli:<pid>`, …). Recorded verbatim, never interpreted.

Each call is one appended `##` entry and one commit
(`chore: handoff <id>`). Files are append-only — history inside a
file is never rewritten.

## `myco resume <id> [--last N]`

Emits the restart context as a compact prompt block:

```text
I-003 in-progress pri:high owner:devin #index memory leak
in: issues/issue-0003-memory-leak.md
blocks:
  T-010 store layer
handoffs:
2026-10-29 · devin · session:cli:abc123
  Done: repro. Next: fix.
open checklist:
  - [ ] add regression test
```

`--json` gives `{item, location, blocks_titles, handoffs[],
open_checklist}` — the same data for machines. `--last N` shows
only the latest N handoff entries (default 10).

`myco item show <id>` surfaces a `handoffs: N` count; `myco check`
keeps `handoffs/` honest — files for removed items are archived to
`handoffs/archive/` by `--fix`, and malformed entries are parse
findings.

For the sepia-side wiring (session hooks calling these verbs), see
[session continuity with sepia](session-continuity.md).
