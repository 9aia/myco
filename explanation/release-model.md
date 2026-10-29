# Release model

Releases are repo automation, not manual steps:
`scripts/release.sh` is triggered by bupha watch on each `main`
push — a debounce window (default 5 min, `MYCO_RELEASE_DEBOUNCE`)
means only the last push in a quiet window actually releases. The
script runs inside a detached worktree and pushes commits and
tags from there, never from the main checkout.

## What a release run does

1. **Diff since the last tag** — crates under `crates/` that
   changed since `git describe --tags` are the release set; the
   `myco` binary re-publishes whenever any crate changed. No
   crate changes, no release.
2. **Per-crate patch bump + publish** —
   `cargo set-version -p <crate> --bump patch`, then
   `cargo publish` with retries.
   Each crate gets its own tag `<crate>-v<ver>` — versions
   diverge independently in the monorepo.
3. **Commit + push** — one `release: bump crate versions` commit,
   pushed to `main` with the tags.
4. **Binary dist** — when `dist` and `gh` are available, a local
   `dist build` produces the shell installer
   (`dist-workspace.toml`, host arch `x86_64-unknown-linux-gnu`
   only — other targets are a TODO for remote workers), uploaded
   to a GitHub Release `myco-v<ver>` with generated notes.

With no tags at all the script exits early — the flow is seeded
by tagging once.

## Constraints

- The crates.io name `myco` is currently held by an unrelated
  project, so `cargo publish` of the CLI package depends on
  resolving that; installs should use
  `cargo install --git https://github.com/9aia/myco` regardless.
- Library crates publish normally under their `myco-*` names.
