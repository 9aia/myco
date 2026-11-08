# myco-driver — `tamias-driver-myco`

The [pavo](https://github.com/9aia/pavo) view driver for a repo's
`project/` ledger — the read-side replacement for `myco-tui`
(retired in the same change; mapping below). Bin name is the tamias
driver contract: `tamias-driver-myco`.

## Verbs

| verb | output |
| --- | --- |
| `manifest` | the `DriverManifest` JSON (name `myco`, views, contributes) |
| `capabilities` | `{"capabilities": ["pavo"], "verbs": ["manifest","serve"], …}` |
| `serve` | ndjson JSON-RPC on stdio — `{"op":"manifest"|"views"|"view"|"query"}` → `{"ok":true,"result":…}` / `{"ok":false,"error":…}` (pavo-protocol v1 wire) |

Install: `cargo build --release -p myco-driver` and put
`target/release/tamias-driver-myco` on `PATH`, or into
`$TAMIAS_HOME/drivers/` (`~/.local/share/tamias/drivers`).

## Which repo does it read?

Every `view`/`query` resolves its own target, first hit wins:

1. **Request params** — `dir` (a `project/` worktree path; `project`
   is accepted as an alias) or `root` (a repo root → `<root>/project`).
   e.g. `pavo view myco:board --param dir=/path/to/repo/project`.
2. **`$MYCO_DIR`** — treated as `dir`.
3. **cwd discovery** — `myco_ops::discover` walks up from the
   driver's cwd for a qualifying `project/` worktree, with the same
   rules `myco` verbs use (`myco.toml`'s `project_dir`, the
   worktree-root cap, …).

`manifest`/`views` never touch project state — pavo can enumerate
the driver from anywhere. When no project resolves, every query
degrades to an `{"error": "…"}` payload inside an `ok:true`
response, which renderers already display inline.

## Architecture

Pure **reader**, in-process: each op opens the store through
`myco_ops::items::open_read_with` (the shared worktree lock, same
discipline as `myco status`), loads the ledger + file items +
handoff summaries + the live link graph (`myco_ops::links::
live_graph` — the same edge set `item show` uses, deliberately not
the persisted redb index), and drops the lock. No `myco serve`
child process, no writes — mutations stay on the `myco` verbs.

`pavo-protocol` is a path dependency on the sibling pavo checkout
(`../../../pavo/crates/pavo-protocol`) until it is published; the
`version` key is the crates.io fallback.

## Views

| view | route | params | covers |
| --- | --- | --- | --- |
| `status` | `/myco/status` | — | kv digest: repo/root/worktree, per-status counts, per-kind counts, warnings |
| `board` | `/myco/board` | — | kv + expandable tree: `TODO.md`/`BACKLOG.md` → sections → items → children |
| `items` | `/myco/items` | `kind`, `status`, `source` | flat table of every item (ledger + file kinds) |
| `item` | `/myco/item` | **`id`** (required) | kv metadata + related-edges table + children table + markdown body (inlined per request — markdown nodes aren't query-bound) |
| `issues` `proposals` `adrs` `plans` | `/myco/<kind>s` | — | one table per file-backed kind |
| `graph` | `/myco/graph` | — | `graph` node over the live edge set; `--format dot`/`mermaid` work |
| `handoffs` | `/myco/handoffs` | — | one row per `handoffs/*.md` |

Contributes: navbar → `/myco/board`, dashboard + statusbar →
`/myco/status`.

## TUI → view mapping (why `myco-tui` could be removed)

| `myco-tui` surface | pavo equivalent |
| --- | --- |
| TODO tab — `##` section headers, nested `- [ ]` children | `myco:board` tree (`TODO.md` → sections → items → children) |
| BACKLOG tab | same tree's `BACKLOG.md` root |
| FILES tab — issues/proposals/plans/adrs grouped under kind headers | `myco:items` (all kinds, `source` column) + per-kind `myco:{issues,proposals,adrs,plans}` tables |
| `Enter` preview pane — id/title/status/pri/owner/labels/epic/ms/created/related/children/note/body | `myco:item` (`--param id=T-001`) — same fields as kv pairs + related table + children table + body markdown |
| preview "related" section (`blocks`, `blocked_by`, `references`, …) | `myco:item` related table; whole graph at `myco:graph` |
| status-bar message / counts | `myco:status` |
| `/` live filter (title/id/labels) | `myco:items --param kind=… --param status=…` (query-side filters; no free-text filter yet) |
| `h`/`l`/`1`/`2`/`3` board tabs | pavo routes — each view is a page |
| `?` help, mouse, `$EDITOR`, `R` reload | n/a — host chrome concerns |
| mutations (`Space` done, `a` add, `dd`, `D` defer, `p` promote, `Alt+↑/↓` reorder, `:` commands) | **not ported** — writes stay on `myco` verbs / `myco serve`; a driver is a read surface |
| `--view org-*` index boards (cross-repo redb universe) | **not ported** — needs the index; candidate follow-up view |

## Queries

`pavo query myco <name> [--param k=v]` answers the same bindings the
views use: `summary`, `ledger`, `items`, `issues`, `proposals`,
`adrs`, `plans`, `item`, `related`, `children`, `body`, `graph`,
`handoffs`.
