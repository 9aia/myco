# Release model

> User-facing install is the tamias pack (`libraries/myco` →
> `tamias install 9aia:myco@<ver>`); the `myco` crates are lib-only
> engines behind `tamias-driver-myco` — there is no cargo-installed
> `myco` binary and **no crates.io publish** (org policy keeps every
> repo but tamias off crates.io; the `myco` name there is held by an
> unrelated project, and path deps like `myco-store-md` were never
> published). This page describes the tag release pipeline that
> feeds the pack.

Releases are manual: `scripts/release.sh` is run by an operator
on `main`. The script's
leading debounce window (default 5 min, `MYCO_RELEASE_DEBOUNCE`)
still applies, and it pushes commits and tags to `main`.

## What a release run does

1. **Diff since the last tag** — release-worthy paths are the
   workspace crates (`crates/`), the tamias pack payload
   (`libraries/`), `xtask/`, the workspace manifests/lockfile, and
   the release script itself. Changes to anything else trigger
   nothing. No release-set changes, no release.
2. **Workspace version bump** — every crate inherits
   `version.workspace`, so `cargo set-version --workspace --bump
   patch` moves them all at once and rewrites the
   `workspace.dependencies` version reqs and `Cargo.lock`
   together. The script then syncs the user-facing version refs:
   `libraries/myco/tamias.yaml` (`version:`) and the install
   examples in `README.md` and `.agents/skills/myco/SKILL.md`
   (`myco@<ver>`).
3. **Commit + tag + push** — one `release: bump myco to <ver>`
   commit, tagged `myco-v<ver>`, pushed to `main` with the tag.
4. **Pack publish (operator)** — the script prints the follow-up:
   `tamias publish libraries/myco` ships `9aia:myco@<ver>` to the
   org registry (needs `TAMIAS_REGISTRY_TOKEN`). That step is
   deliberately outside the script — publishing is a
   credentials-gated, registry-side action.

With no tags at all the script exits early — the flow is seeded
by tagging once.

## Constraints

- There is no crates.io publish path and no per-crate tag scheme —
  the whole workspace shares one version and one `myco-v<ver>`
  tag.
- Only the tamias pack is user-facing; `myco-tui`'s standalone
  binary and `tamias-driver-myco` are dev conveniences installed
  via `cargo xtask install`, not release artifacts.
- Prebuilt binary distribution (GitHub Releases + cargo-dist —
  `dist-workspace.toml` is configured but idle) is a future option
  tracked as T-041, not part of the release flow.

## CI and docs deploys

There are no GitHub Actions workflows; the gate is the peregrin
`ci-gate` campaign (the gate is mid-migration back to bupha):

- **Push to `docs`** runs `scripts/deploy-docs.sh`, which
  rebuilds the site and pushes `gh-pages` — Pages is a legacy
  branch deploy, so that push is the whole release of the site.
- **Push to `main`** is gated by the ci-gate campaign
  (`scripts/ci.sh` = `check && test` + the coverage floor) and
  locally by the `pre-push` hook (`cargo xtask check`).
