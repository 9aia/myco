+++
id = "P-005"
title = "link index — DAG edges, fingerprint invalidation, bounded GC"
status = "open"
created = "2026-10-31"
priority = "medium"
labels = ["index"]
blocks = ["I-019"]
+++

## Problem

Cross-item navigation — the TUI related-files view (I-019),
`item show` context, future `myco serve` queries — needs to
answer "what is attached to item X?" Today that's a scan of
every `project/` file per query: fine at 30 items, O(repo) per
keystroke at scale.

## Proposal

A **link index**: a derived, disposable cache in `myco-index`
(redb impl) mapping item → edges, built incrementally and
invalidated by content fingerprint.

### Edge model — a DAG that tolerates cycles

| Edge | Source | Authority |
| --- | --- | --- |
| `handoff` | `handoffs/<ID>.md` filename | exact |
| `blocks` / `blocked_by` | frontmatter `blocks:` | explicit |
| `supersedes` / `superseded_by` | frontmatter | explicit |
| `references` / `referenced_by` | `X-NNN` tokens in bodies | derived |

Edges stored as `(src, dst, kind)` with the source file as
provenance; a reverse table `dst → src` gives O(1)
`related(id)`. blocks/supersedes form a DAG on healthy ledgers,
but cyclic input must not break it — traversals carry a visited
set and a depth cap; no acyclicity precondition.

### Incremental invalidation — files as signals

Each `project/` file gets a fingerprint (xxh3 of contents;
mtime+size precheck). On open/reload:

1. Diff live fingerprints vs stored → dirty set.
2. Re-derive edges for dirty files only — pure
   `refs::extract(body, meta)` in `myco-core`, no I/O.
3. Swap that file's outgoing edges + stamp in one redb txn —
   crash-safe, never half-applied.

Unchanged files never re-parse. The index is a signal graph:
file content is the input, edges are the memoized output.

### GC & bounds

- **Sweep**: entries whose source file vanished are deleted;
  dangling edges (dst id gone) filtered at query time,
  compacted on sweep.
- **Bounds**: mention scan capped (64 KiB/file, N edges/file);
  TUI query cache is an LRU. Index stays O(edges);
  `myco index rebuild` is the escape hatch.
- Ledger snapshots ride the same fingerprints → `status`/TUI
  stop re-parsing unchanged docs.

### Ownership

- `myco-core` — `refs::extract` pure scanner.
- `myco-index` (port) — `record`, `related`, `dirty`, `sweep`.
- `myco-index-redb` — `files`/`edges`/`rev_edges` tables next to
  the existing snapshots.
- `myco-ops`, `myco-tui`, `serve` — consumers.

### Phases

1. Pure `extract` + in-memory graph in the TUI — proves the UX,
   still O(n) once per session.
2. redb persistence + fingerprint invalidation + sweep.
3. `item show` / `serve` consumers; optional explicit `relates:`
   field only if derived edges prove insufficient.

## Non-goals

Cross-repo edges (org stays per-repo), cycle *prevention*
(detection only), file-spec changes.
