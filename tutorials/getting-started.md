# Getting started

Install the `myco` CLI, then turn a repo's `project/` directory
into a work ledger — tasks, issues, proposals, plans, ADRs,
handoffs — that lives on its own orphan branch.

## Install

The `myco` command is a tamias `bins[]` pack (`libraries/myco`) —
a generated shim → `tamias bin exec` → `tamias-driver-myco cli`
→ the engine lib. There is no cargo-installed `myco` binary. With
[tamias](https://github.com/9aia/tamias) installed:

```sh
tamias install 9aia:myco@<version>    # org registry (needs TAMIAS_REGISTRY_TOKEN)
```

Or the npm launcher — a thin shim that finds the `myco` bin on
PATH and bootstraps tamias + the pack on first run:

```sh
npm install -g 9aia/myco
# npm ≥ 12 gates git fetches: npm install -g --allow-git=root 9aia/myco
```

From a checkout of this repo:

```sh
cargo xtask pack && tamias install --allow-drivers ./libraries/myco
cargo xtask install        # tamias-driver-myco → ~/.cargo/bin (dev convenience)
```

To also get the
[pavo](https://github.com/9aia/pavo) view driver
(`tamias-driver-myco`, see below):

```sh
cargo install --git https://github.com/9aia/myco myco-driver
```

## Set up a repo

```sh
cd your-repo
myco init
```

`init` creates the orphan `project` branch, mounts it as a
`project/` worktree, seeds `meta.toml`, `TODO.md`, `BACKLOG.md`,
`DONE.md`, `index.md` and the item dirs (`issues/`, `proposals/`, `plans/`,
`adrs/`, `handoffs/`), seeds a commented `[[kinds]]` prelude into
`myco.toml` at the repo root (only when absent), adds `project/`
to `.gitignore` on your
code branch, and writes a managed _Agent awareness_ block into
`AGENTS.md`. It is idempotent — run it again and a healthy
worktree is verified, a damaged one repaired.

## Your first item — a full lifecycle

Add a task; myco allocates `T-001` (the `t` counter in `meta.toml`)
and appends one line to `project/TODO.md`:

```sh
myco todo add "write the tutorial" --pri high --label docs
# → added T-001 open pri:high #docs write the tutorial
```

```markdown
- [ ] T-001 write the tutorial · pri:high · created:2026-10-29 · #docs
```

Everything after the title is `·`-separated tokens — `status:`,
`pri:`, `owner:`, `created:`, `#label`, `blocks:`, `links:`, `epic:`,
`ms:`, `due:`, `est:`, `cycle:`, `note:` (see
[the file spec](/explanation/file-spec) for the full grammar). Nest a
sub-task under it — children are `- [ ]` lines indented two spaces
under the parent, full items with the same state machine, and
`done`/`defer` on a parent act on the whole subtree:

```sh
myco todo add "review it" --parent T-001
```

Move it through the state machine:

```sh
myco item set T-001 --status in-progress --owner devin
myco view board
myco status            # ledger digest
```

Statuses are `open → claimed → in-progress → done`, with `blocked`
and `deferred` off the main path — see
[the file spec](/explanation/file-spec#state-machine) for the legal
transitions. `item set` walks the machine one step at a time; an
illegal jump errors and tells you the valid targets. Park something
instead of finishing it:

```sh
myco todo defer T-002        # → deferred; the line moves to BACKLOG.md
myco backlog promote T-002   # → open; back to TODO.md
```

Leave a handoff, then finish:

```sh
myco handoff T-001 --note "Done: draft. Next: screenshots." --actor devin
myco resume T-001       # minimal restart context
myco todo done T-001    # the line moves to DONE.md — still browsable
myco check              # validate files, state machine, index drift
```

`handoff` appends a dated entry to `handoffs/T-001.md`; `resume`
prints the item, its `blocks:` deps, latest handoffs, and open
checklist. Done tasks move to `DONE.md` — the done archive keeps
completed work browsable (`myco done list`, `myco item show`) instead
of vanishing; the `project` branch history is still the record.

Every mutating verb is one atomic commit on the `project` branch —
the branch history is the audit trail (`git -C project log
--oneline` shows them all). `--no-commit` (or the `MYCO_NO_COMMIT`
env var) opts out.

## The interactive board

`myco ui` opens a full-screen board over `TODO.md`, `BACKLOG.md`,
and the file-backed kinds (issues, proposals, plans, ADRs) — the
human cockpit while agents drive the verbs. Every mutation runs
through the same ops the CLI uses, so a keypress that changes the
ledger lands as the same atomic commit (`--no-commit` opts out
here too). The board auto-reloads when the ledger changes on disk
— an OS filesystem watcher plus a content fingerprint catch other
`myco` invocations, `git pull`, and hand-edits — no keypress
needed; `R` forces one anyway.

| Key                            | Action                                       |
| ------------------------------ | -------------------------------------------- |
| `Tab` / `Shift-Tab`, `1` / `2` / `3`, `h` / `l` | cycle / pick TODO / BACKLOG / FILES tabs |
| `j` / `k`, `gg` / `G`, `{`/`}` | move · ends · section jumps                  |
| `Enter`, `→`                  | item preview — `j`/`k` scroll, `←`/esc back   |
| `←`                           | back — pops the drill-down stack (esc-like in preview, no-op on the board) |
| `a`                            | add an item (TODO/BACKLOG only)              |
| `Space` / `dd`, `D`, `p`       | done · defer to BACKLOG · promote to TODO (done/undo ask `y`/`n`) |
| `u` / `U`                      | undo / redo ledger commits — `U` repeats while the redo stack lasts (asks `y`/`n`) |
| `Alt+j` / `Alt+k`              | reorder among siblings                       |
| `/`, `F`                       | live filter · clear it                       |
| `:`                            | command line — `:q` `:w` `:e` `:<id>` `:set` |
| `E`, `R`, `?`                  | `$EDITOR` · reload · keybind help            |
| `q`, `Ctrl-C`                  | quit                                         |
| click, wheel                   | select item / tab · scroll                   |
| double-click                   | open item preview                            |

`--view org-board` (or any `source = "index"` view) swaps the repo
ledger for the cross-repo index universe — rows are `repo:ID` and
verbs write the owning repo's worktree.

The TUI is a default cargo feature of the engine — the pack build
includes it, and since the crates are lib-only there is no
`--no-default-features` install path; feature selection is a
pack-build concern.

## Visual views — pavo

For a read-only visual surface (and cross-repo org boards),
[pavo](https://github.com/9aia/pavo) renders the ledger through
`tamias-driver-myco` (the `myco-driver` crate in this repo):

```sh
cargo install --git https://github.com/9aia/myco myco-driver
cargo install --git https://github.com/9aia/pavo pavo   # the host, once
cd your-repo
pavo view myco:board     # TODO/BACKLOG tree — also myco:status,
                         # myco:items, myco:issues, …
pavo open myco:board     # same views in the pavo TUI
```

Run `pavo` inside the repo (or pass `--param
dir=…/project`) so the driver knows which ledger to open. The
driver's reads are view queries; writes ride the same engine
verbs — `tamias-driver-myco run <verb>` (or the driver's `ops`
manifest / `cli.<verb>` queries) reaches the full `myco` surface,
commit-per-mutation included. `myco ui` stays the interactive
path.

## Agent self-discovery

```sh
myco manifest          # capabilities + protocol version (JSON)
myco schema            # the file/state spec as JSON Schema
myco status --prompt   # token-sized digest for a context window
```

Every verb accepts `--json`. `myco check` validates the ledger —
file schemas, state machine, index drift — and `myco check --fix`
repairs what it safely can.

## Where next

- [Run the daily agent loop](/how-to/agent-loop) — how an agent
  drives these verbs.
- [Set up myco on a repository](/how-to/init-repo) — discovery,
  overrides, non-standard layouts.
- [The worktree model](/explanation/worktree-model.md) — what is
  actually on disk and why.
- [CLI reference](/reference/cli.md) — the full verb surface.
