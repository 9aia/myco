# Getting started

Install the `myco` CLI, then turn a repo's `project/` directory
into a work ledger — tasks, issues, proposals, plans, ADRs,
handoffs — that lives on its own orphan branch.

## Install

The crates.io name `myco` belongs to an unrelated project —
install from the repo:

```sh
cargo install --git https://github.com/9aia/myco myco
```

The trailing `myco` selects the package — the repo is a workspace,
so cargo needs to know which member to install. To also get the
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
code branch, and writes a managed _Agent awareness_ block into
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

`myco` is verb-first — agents drive it — but humans get a visual
surface through [pavo](https://github.com/9aia/pavo), the org's
view host. `tamias-driver-myco` (the `myco-driver` crate in this
repo, installed above) serves the ledger as pavo views:

```sh
cargo install --git https://github.com/9aia/pavo pavo   # the host, once
cd your-repo
pavo view myco:board     # TODO/BACKLOG tree — also myco:status,
                         # myco:items, myco:issues, …
```

Run `pavo view` inside the repo (or pass `--param
dir=…/project`) so the driver knows which ledger to open. The
driver is read-only — every mutation stays on a `myco` verb, and
each verb lands as the same atomic commit (`--no-commit` opts
out).

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

- [Your first item](tutorials/first-item.md) — a guided walk
  through a full task lifecycle.
- [Set up myco on a repository](how-to/init-repo.md) — discovery,
  overrides, non-standard layouts.
- [The worktree model](explanation/worktree-model.md) — what is
  actually on disk and why.
- [CLI reference](reference/cli.md) — the full verb surface.
