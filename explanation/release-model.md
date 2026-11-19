---
url: /myco/explanation/release-model.md
---
# Release model

> User-facing install is the tamias pack (`libraries/myco` →
> `tamias install 9aia:myco@<ver>`); the `myco` crates are lib-only
> engines behind `tamias-driver-myco` — there is no cargo-installed
> `myco` binary and **no crates.io publish** (org policy keeps every
> repo but tamias off crates.io; the `myco` name there is held by an
> unrelated project, and path deps like `myco-store-md` were never
> published). This page describes the tag release pipeline that
> feeds the pack.

Releases are manual: an operator on `main` runs
`tarsi-serve release --dir .` — dry-run by default, `--yes` as the
explicit opt-in. The per-repo `scripts/release.sh` shim was
removed org-wide; the command talks to tarsi directly.

The org release plane is **tarsi** (`tarsi-serve` ops — project
ADR-0005); repo policy lives in `tarsi.toml`.

## What a release run does

`tarsi-serve release` composes the pipeline — plan → changelog →
bump → tag → push — over `tarsi.toml`:

1. **Plan** — commits since the last `myco-v*` tag are
   classified (conventional commits) and attributed to release
   units; the workspace shares one version, so the plan is a
   single-unit lockstep bump. No release-worthy changes, no
   release.
2. **Bump** — manifests, `Cargo.lock`, and the user-facing
   version refs (`libraries/myco/tamias.yaml`, the install
   examples in `README.md` and `.agents/skills/myco/SKILL.md`)
   are rewritten inside the release commit via
   `[release.sync_refs]` — the tagged tree is self-consistent.
3. **Tag + push** — one release commit tagged `myco-v<ver>`,
   pushed to `main` with the tag.
4. **Pack publish (operator)** — after the tag the operator runs
   `tamias publish libraries/myco` to ship `9aia:myco@<ver>` to
   the org registry. That step is deliberately outside the
   release — publishing is a credentials-gated, registry-side
   action.

## Constraints

* There is no crates.io publish path and no per-crate tag scheme —
  the whole workspace shares one version and one `myco-v<ver>`
  tag.
* Only the tamias pack is user-facing; `myco-tui`'s standalone
  binary and `tamias-driver-myco` are dev conveniences installed
  via `cargo xtask install`, not release artifacts.
* Prebuilt binary distribution (GitHub Releases + cargo-dist —
  `dist-workspace.toml` is configured but idle) is a future option
  tracked as T-041, not part of the release flow.

## CI and docs deploys

There are no GitHub Actions workflows; the gate is the peregrin
`ci-gate` campaign (the gate is mid-migration back to bupha):

* **Push to `docs`** runs `scripts/deploy-docs.sh`, which
  rebuilds the site and pushes `gh-pages` — Pages is a legacy
  branch deploy, so that push is the whole release of the site.
* **Push to `main`** is gated by the ci-gate campaign
  (`scripts/ci.sh` = `check && test` + the coverage floor) and
  locally by the `pre-push` hook (`cargo xtask check`).
