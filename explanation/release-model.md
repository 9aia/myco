---
url: /myco/explanation/release-model.md
---
# Release model

Releases are manual: `scripts/release.sh` is run by an operator
on `main` — the old trigger (bupha watch on each `main` push) is
retired; CI is now the peregrin `ci-gate` campaign. The script's
leading debounce window (default 5 min, `MYCO_RELEASE_DEBOUNCE`)
still applies, and it pushes commits and tags to `main`.

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

* The crates.io name `myco` is currently held by an unrelated
  project, so `cargo publish` of the CLI package depends on
  resolving that; installs should use
  `cargo install --git https://github.com/9aia/myco` regardless.
* Library crates publish normally under their `myco-*` names.

## CI and docs deploys

There are no GitHub Actions workflows; the gate is the peregrin
`ci-gate` campaign (bupha watch is retired):

* **Push to `docs`** runs `scripts/deploy-docs.sh`, which
  rebuilds the site and pushes `gh-pages` — Pages is a legacy
  branch deploy, so that push is the whole release of the site.
* **Push to `main`** is gated by the ci-gate campaign
  (`scripts/ci.sh` = `check && test` + the coverage floor) and
  locally by the `pre-push` hook (`cargo xtask check`).
