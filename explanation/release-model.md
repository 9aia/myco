# Release model

> User-facing install is the tamias pack (`libraries/myco` →
> `tamias install 9aia:myco@<ver>`); the `myco` crates are lib-only
> engines behind `tamias-driver-myco` — there is no cargo-installed
> `myco` binary. This page describes the crate/tag release pipeline
> that feeds it.

Releases are manual: `scripts/release.sh` is run by an operator
on `main`. The script's
leading debounce window (default 5 min, `MYCO_RELEASE_DEBOUNCE`)
still applies, and it pushes commits and tags to `main`.

## What a release run does

1. **Diff since the last tag** — the release set is a *fixed*
   list of five crates: `myco-core`, `myco-git`, `myco-ops`,
   `myco-shell`, `myco-cli` (`myco-ops` carries the config
   loader and index universe since the `myco-config*` crates
   were retired). Only changes under `crates/<one-of-those>/`
   count; changes to `myco-store*`, `myco-index*`, `myco-tui`,
   `myco-driver`, or `myco-testkit` trigger nothing. The `myco`
   binary re-publishes whenever any of the five changed. No
   release-set changes, no release.
2. **Per-crate patch bump + publish** —
   `cargo set-version -p <crate> --bump patch`, then
   `cargo publish` with retries, in dependency order
   (`myco-core myco-git myco-ops myco-shell myco`).
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
  resolving that; and since T-006 the `myco` crates ship no
  `[[bin]]` — installs go through the tamias pack
  (`tamias install 9aia:myco@<ver>`), not `cargo install`.
- Only the six release-set crates publish — the rest of the
  workspace (`myco-ops`, `myco-store*`, `myco-index*`, `myco-tui`,
  `myco-driver`, `myco-testkit`) is internal; `myco-driver` still
  builds a real `tamias-driver-myco` bin for dev installs.

## CI and docs deploys

There are no GitHub Actions workflows; the gate is the peregrin
`ci-gate` campaign (the gate is mid-migration back to bupha):

- **Push to `docs`** runs `scripts/deploy-docs.sh`, which
  rebuilds the site and pushes `gh-pages` — Pages is a legacy
  branch deploy, so that push is the whole release of the site.
- **Push to `main`** is gated by the ci-gate campaign
  (`scripts/ci.sh` = `check && test` + the coverage floor) and
  locally by the `pre-push` hook (`cargo xtask check`).
