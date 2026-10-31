+++
id = "I-019"
title = "TUI: ITEMS tab → FILES, related-files view, {/} kind jumps"
status = "open"
created = "2026-10-31"
priority = "medium"
labels = ["tui"]
+++

`Board::Files` is labeled `ITEMS` in the tab bar, but every
kind in myco is an item — including TODO/BACKLOG lines. Rename
the tab `FILES` (the file-backed kinds: issues/proposals/plans/
adrs) and turn it into a real navigation surface:

- `{` / `}` — jump between kind section headers (vim-idiomatic;
  optional `gi`/`gp`/`gl`/`ga` go-to-kind)
- **related-files view** — for the item under the cursor, show
  attachments: its `handoffs/<ID>.md` (id-keyed), `blocks` /
  `supersedes` edges, and files whose bodies mention the id.
  Surface in the preview pane or a `gr` popup.

The mention-scan half of "related" should ride the link index
(proposal) rather than rescanning `project/` per keystroke; a
naive in-memory build is acceptable for v1.

Related: I-013 — TUI hardening covers this board's stale
`files` data (loaded once, never reloaded) and missing
force-quit; fix alongside the rename.
