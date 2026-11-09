+++
id = "P-013"
title = "typed item links + clickable TUI related ids"
status = "open"
created = "2026-11-09"
priority = "medium"
+++

## Problem

Two gaps in how the ledger links items and how surfaces render them.

1. **No typed artifact association.** `Item` knows `blocks`,
   `supersedes`, `superseded_by`; everything else is a prose
   `X-NNN` mention → a weak `references` edge. "The plan for this
   task", "the proposal that spawned it" have no declared form —
   they ride the same edge kind as any incidental citation, so
   `related` can't distinguish *attachment* from *mention*.
2. **Links aren't actionable in the TUI.** `myco ui`'s preview
   pane renders the related section as plain text
   (`myco-tui/src/render.rs` `related_lines`); clicks inside the
   preview are ignored (mouse is handled in `Mode::Normal` on
   board rows only). The drill-down stack already exists — a
   related id should open that item's preview on click.

## Proposal

### 1. `links:` — declared, kind-agnostic association

- `Item.links: BTreeSet<ItemId>` — emitted as `links:` in
  item-file frontmatter and a `links:<id>[,<id>]` token in the
  TODO/BACKLOG line grammar (same shape as `blocks:`).
- New `EdgeKind::Associated` (`associated`), extracted from
  `links:` in `refs::extract`; mirrors to itself — "associated"
  reads the same in both directions, so `related` shows one
  group for incoming and outgoing.
- Verbs: `myco item set <ID> --link <ID> --unlink <ID>`
  (mirroring `--blocks`/`--unblock`); `NewItemMeta.links` for
  `todo add` / `proposal new --link`.
- Surfaces: `item show` related section gains `associated:`;
  `--json` inherits it; the persisted index picks the kind up on
  rebuild (unknown-kind forward compat already exists).
- `check`: dangling `links:` is an **error** per ADR-0002
  (declared ref) — add beside `dangling-blocks`; also close the
  ADR's noted gap (`supersedes:`/`superseded_by:` unchecked).

### 2. Clickable related ids in `myco ui`

- `draw_preview` emits each related id as its own span and
  records `(ItemId → Rect)` on `Model` per frame (same pattern
  as `board_area`/`tabs_area`).
- `mouse()` handles `Mode::Preview`: hit-test link rects →
  resolve the id → push the drill stack → open the target's
  preview (file-backed kinds resolve through `m.files`).
- Follow-on (not required): linkify `X-NNN` tokens inside
  rendered file bodies.

`myco view`/pavo untouched — no click concept there; ids are
already copy-pasteable.

## Phases

1. Core + grammar + verbs: `EdgeKind::Associated`, `Item.links`,
   frontmatter/line-token parse+emit, `refs::extract`,
   `item set --link/--unlink`, `NewItemMeta.links`, serve/batch
   flag plumbing.
2. TUI clickable related ids.
3. `check` dangling-link validation + docs/skill updates.

## Open question

Field/edge naming: `links` chosen (short, kind-agnostic);
`artifacts`/`related` were the alternatives.
