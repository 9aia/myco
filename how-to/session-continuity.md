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
titles of its `blocks:` dependencies, the latest handoff entries,
and the unchecked `- [ ]` checklist items for file-backed items —
the single-item counterpart of `myco status --prompt`. Human
output is a compact prompt-ready block; `--last N` caps the
entries shown (default 10).

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

When an item is removed — done tasks leave `TODO.md` — its
handoff file becomes orphaned. `myco check` reports it and
`myco check --fix` moves it to `handoffs/archive/`: the live dir
stays clean, the knowledge survives, and `myco resume` on an
archived item still finds it.
