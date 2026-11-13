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
| `ops` | the engine ops manifest — every `myco` verb as a `cli.<verb>` wire query, derived from `myco::registry::VERBS` |
| `run <argv>` | engine passthrough — `tamias-driver-myco run status` ≡ `myco status`; also the child side of `cli.*` wire queries |
| `cli <myco argv>` | the tamias `bins[]` shim verb — `tamias bin exec` calls `cli --dir <pkg> --name myco <argv>`; strips the driver flag pair, runs `myco::run_from` with inherited stdio (this IS the `myco` command — T-006) |
| `mcp` | *(feature `mcp`)* the ops manifest as MCP tools on stdio |
| `serve` | ndjson JSON-RPC on stdio — `{"op":"manifest"|"views"|"view"|"query"}` → `{"ok":true,"result":…}` / `{"ok":false,"error":…}` (pavo-protocol v1 wire) |

The `myco` command ships as the tamias pack in `libraries/myco/`
(`bins: [{name: myco, shim: cli, driver: myco}]`) — `cargo xtask
pack` stages this binary into `drivers/bin/<triple>/` and `tamias
install ./libraries/myco` registers it under `<tamias-home>/drivers/`
and deploys the shim. Engine e2e tests live in `tests/` and spawn
`tamias-driver-myco run <argv>` — the same path `cli.*` queries and
the shim take.

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

`pavo-protocol` is a git dependency on the pavo repo so installs
work without a sibling checkout; the `version` key is the
crates.io fallback once it is published. For local pavo
development, patch it back to the path dep (or `[patch]` it in
`.cargo/config.toml`).

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
| mutations (`Space` done, `a` add, `dd`, `D` defer, `p` promote, `Alt+↑/↓` reorder, `:` commands) | **ported via `invoke`** — action ids are verb names dispatched through the engine path; `submit` adapts verb-named forms. See Mutations below |
| `--view org-*` index boards (cross-repo redb universe) | **not ported** — needs the index; candidate follow-up view |

## Queries

`pavo query myco <name> [--param k=v]` answers the same bindings the
views use: `summary`, `ledger`, `items`, `issues`, `proposals`,
`adrs`, `plans`, `item`, `related`, `children`, `body`, `graph`,
`handoffs`.

## Mutations

Writes ride the pavo interaction protocol (ADR-0006): the `invoke`
wire op dispatches an action id — which is a `myco` verb name —
through the same engine path as `cli.<verb>` queries, so commits,
actor trailers, and the state machine behave identically. `submit`
remains as a thin adapter for verb-named forms.

Actions are declared at three levels (`views.rs`):

- **global** — `todo.add`, `backlog.add`, `todo.done`, `item.set`,
  `handoff`, `next`, `undo`, `redo`, … (manifest `actions[]` +
  keybindings like `a`/`dd`)
- **row** — per-item actions on board/tree rows (`item.set --status`,
  claim/release, labels)
- **item node** — actions on the `myco:item` view itself

Mutating actions carry `confirm` danger gates; hosts render them as
buttons, palette entries, or keybindings on all three surfaces
(CLI/TUI/web).
