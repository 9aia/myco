---
url: /myco/getting-started.md
---
# Getting started

Install the `myco` CLI, then turn a repo's `project/` directory
into a work ledger — tasks, issues, proposals, plans, ADRs,
handoffs — that lives on its own orphan branch.

## Install

The crates.io name `myco` belongs to an unrelated project —
install from the repo. Pick one:

```sh
npm install -g 9aia/myco      # easiest — first `myco` run builds via cargo
cargo install --git https://github.com/9aia/myco myco
```

On npm ≥ 12, git fetches are gated — add `--allow-git=root`:
`npm install -g --allow-git=root 9aia/myco`.

The npm package is a thin shim — the first `myco` run builds the
Rust binary via `cargo install --git … myco` (there's no
postinstall; npm breaks lifecycle-script cwds on git installs),
so **Rust + cargo are still required** — the first build takes a
minute or two.

The trailing `myco` on the cargo command selects the package —
the repo is a workspace, so cargo needs to know which member to
install. To also get the
[pavo](https://github.com/9aia/pavo) view driver
(`tamias-driver-myco`, see below):

```sh
cargo install --git https://github.com/9aia/myco myco-driver
```

From a checkout of this repo: `cargo xtask install` builds and
installs the release binary to `~/.cargo/bin/myco`.

## Set up a repo

```sh
cd your-repo
myco init
```

`init` creates the orphan `project` branch, mounts it as a
`project/` worktree, seeds `meta.toml`, `TODO.md`, `BACKLOG.md`,
`index.md` and the item dirs (`issues/`, `proposals/`, `plans/`,
`adrs/`, `handoffs/`), adds `project/` to `.gitignore` on your
code branch, and writes a managed *Agent awareness* block into
`AGENTS.md`. It is idempotent — run it again and a healthy
worktree is verified, a damaged one repaired.

## First items

```sh
myco todo add "ship the thing" --pri high --label release
myco item set T-001 --status in-progress
myco status            # ledger digest
myco todo done T-001   # the line leaves TODO.md — git history keeps it
```

Every mutating verb is one atomic commit on the `project` branch —
the branch history is the audit trail. `--no-commit` (or the
`MYCO_NO_COMMIT` env var) opts out.

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
| `1` / `2` / `3`, `h` / `l`     | TODO / BACKLOG / FILES tabs                  |
| `j` / `k`, `gg` / `G`, `{`/`}` | move · ends · section jumps                  |
| `Enter`, `→`                  | item preview — `j`/`k` scroll, `←`/esc back   |
| `←`                           | back — pops the drill-down stack (esc-like in preview, no-op on the board) |
| `a`                            | add an item (TODO/BACKLOG only)              |
| `Space` / `dd`, `D`, `p`       | done · defer to BACKLOG · promote to TODO    |
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

The TUI is a default cargo feature — for a lean binary without
it, install with `--no-default-features`:

```sh
cargo install --git https://github.com/9aia/myco myco --no-default-features
```

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
driver is read-only — mutations stay on `myco` verbs (or `myco
ui`).

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

* [Your first item](tutorials/first-item.md) — a guided walk
  through a full task lifecycle.
* [Set up myco on a repository](how-to/init-repo.md) — discovery,
  overrides, non-standard layouts.
* [The worktree model](explanation/worktree-model.md) — what is
  actually on disk and why.
* [CLI reference](reference/cli.md) — the full verb surface.
