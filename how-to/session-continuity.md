# Session continuity with sepia

sepia owns session lifecycle; myco owns work memory. The boundary
is two verbs — no sepia code lives in this repo, and no myco code
in sepia. Each side just calls the other's CLI.

## The pattern

Session end (sepia hook) — leave a resumable note on the item you
were working, tagged with the sepia session id:

```sh
myco handoff T-012 \
  --actor devin \
  --session <sepia-session-id> \
  --note "Done: store layer landed. Next: wire verbs. Open: archive semantics for ADRs."
```

`--note -` reads stdin, so a hook can pipe a generated summary.
`--session` accepts any id string — a sepia session id, a
`cli:<pid>:<ts>` fallback, whatever wrote the entry. It's
provenance only; myco records it and never interprets it.

Session start — pull the minimal restart context into the prompt:

```sh
myco resume T-012 --json
```

`resume` returns the item (status, priority, owner, labels), the
titles of its `blocks:` dependencies and `links:` associations,
the latest handoff entries (`handoffs` + `handoffs_total`), and
the unchecked `- [ ]` checklist items for file-backed items —
the single-item counterpart of `myco status --prompt`. Human
output is a compact prompt-ready block; `--last N` caps the
entries shown (default 10).

## Abnormal endings — `myco interrupt`

A session that dies mid-work shouldn't leave items claimed
forever. `myco interrupt` is the teardown verb:

```sh
myco interrupt T-012 --note "session crashed" --actor devin
myco interrupt --session cli:abc123        # every item claimed under it
```

With an id, the item walks a legal step back to `open` (from
`in-progress`/`blocked`/`claimed`) and gains an `interrupted`
handoff entry in the same commit — `myco item show` and the board
flag it with `⚡` until real work resumes. Without an id,
`--session <id>` reverts every item whose claim was tagged with
that session (claims made via `item set --session`/`--claim`).

## Where handoffs live

`handoffs/<ID>.md` — one append-only file per item, uniform across
every kind:

```markdown
+++
item = "T-012"
+++

# Handoffs — T-012

## 2026-10-29 · devin · session:cli:abc123

Done: store layer landed. Next: wire verbs. Open: archive semantics for ADRs.
```

Entries are never rewritten — each `myco handoff` is one appended
`## <date> · <actor> · session:<id>` heading and one atomic commit
(`chore: handoff T-012`) on the `project` branch, so the branch
history is the audit trail.

When an item is done it doesn't disappear — the whole subtree
moves to `DONE.md`, the done archive. `myco done list` browses it
with the same filters as the other lists, and `myco item show`
still resolves it, so its handoff file stays live. A handoff only
becomes orphaned when the item is truly gone; `myco check`
reports it and `myco check --fix` moves it to
`handoffs/archive/`: the live dir stays clean, the knowledge
survives, and `myco resume` on an archived item still finds it.
