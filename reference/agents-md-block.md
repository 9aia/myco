# Managed `AGENTS.md` blocks

The `<tool>:agent-awareness` convention — how multiple tools each
maintain a section of a repo's `AGENTS.md` without clobbering human
prose or each other. Normative; **myco is the reference
implementation** (`crates/myco-cli/src/agentmd.rs`). sturn, kuda,
bupha, and mimus blocks are deployed against it.

Keywords **MUST** / **MUST NOT** / **SHOULD** / **MAY** read as in
RFC 2119.

## Purpose

`AGENTS.md` at the repo root is agent-facing operating
instructions. Each participating tool owns exactly one
marker-delimited block; everything outside markers is human-owned
and untouchable. A tool's init writes its block, its check
verifies it, and syncs are idempotent — safe to re-run forever.

## The block

```markdown
<!-- <tool>:agent-awareness -->

## <Tool> awareness

- <tool-specific bullets>

<!-- /<tool>:agent-awareness -->
```

- `<tool>` is the implementer's canonical lowercase name (`myco`,
  `sturn`, `kuda`…). The marker pair is the ownership boundary —
  everything between them MAY be rewritten by the owning tool at
  any time.
- Markers **MUST** sit on their own lines. Detection tolerates
  leading/trailing whitespace; canonical emission has none.
- The section heading **SHOULD** be `## <Tool> awareness` —
  uniqueness across tools is what matters (see *upgrade* below).
- Block content is tool-internal: markers are the contract, the
  prose inside is the tool's own business.

## Ownership

- A tool **MUST** rewrite only inside its own marker pair.
- All other content — human prose and foreign tools' marked
  blocks — is opaque: preserved verbatim, never reordered, never
  merged, never reformatted.
- Blocks coexist in any order. A new block appends at end of
  file; a tool **MUST NOT** move its own or others' blocks.
- A tool **MUST NOT** emit a second marker pair, emit markers
  inside foreign blocks, or leave stray markers behind.

## Sync

The write path (`init`, `check --fix`, any rollout) applies one
merge:

1. **Own complete marker pair** → replace the span
   `[BEGIN … END]` inclusive with the canonical block.
2. **No intact pair** → drop stray own-markers; if the file holds
   the tool's claimed heading (an *unmarked* `## <Tool>
   awareness` section), replace that section through the next
   heading of the same-or-higher level — the upgrade path for
   pre-convention or hand-written content.
3. **Neither** → append the canonical block at end of file.

Emission rules:

- Exactly one blank line on each side of the block.
- File ends with exactly one newline.
- Bytes outside the block survive byte-for-byte — no other
  whitespace normalization.
- Sync of already-canonical input is a **no-op** (no write) —
  this is what makes it idempotent.

## States & `check --fix`

Every implementation exposes the same four-state read:

| state | meaning | `check --fix` |
|---|---|---|
| `missing` | no `AGENTS.md` | creates file + block |
| `absent` | file exists, nothing block-shaped | appends block |
| `drift` | stray markers, handwritten section, or non-canonical content | rewrites to canonical |
| `ok` | canonical | no finding, no write |

`check` **MUST** surface non-`ok` as a fixable finding. Detection
is a simulated sync: markers present and `sync(content) ==
content` (modulo the trailing newline) → `ok`.

## Multi-tool coexistence

- Foreign marked regions are opaque to *all* of a tool's logic —
  including its upgrade scan. When looking for its claimed
  heading, a tool skips headings inside other tools' marker
  pairs.
- The `## <Tool> awareness` naming rule is what prevents upgrade
  collisions: two tools scanning the same unmarked file must
  claim different headings.
- Deployed blocks may interleave with prose arbitrarily — no tool
  may assume adjacency or ordering.

## Deployment

- Writes happen on the repo's checked-out branch (the block lives
  beside `AGENTS.md`, not on any tool-owned branch), committed
  separately from other init artifacts. myco's message:
  `chore: manage AGENTS.md agent-awareness block`.
- Multi-repo rollout is an implementer concern (myco: `myco org
  init` applies the block across registered repos).

## Implementation notes

- myco's deployed heading is `## Agent awareness` — predates the
  `<Tool> awareness` rule; it is the one allowed exception, and
  its upgrade matcher is deliberately scoped to that heading so
  no other tool can accidentally adopt it.
- myco exposes the state over the wire: `check --json` reports
  `agents_md: missing|absent|drift|ok` alongside the finding.
