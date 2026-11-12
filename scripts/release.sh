#!/usr/bin/env bash
# Manual release — run by an operator on main (bupha watch, the old
# push trigger, is retired; releases are operator-run).
# The leading sleep is the debounce window: only the last invocation
# in a quiet window releases. Commits/tags are pushed to main.
#
# crates.io publish is retired: every myco crate is an internal
# lib-only engine — the `myco` name on crates.io is held by an
# unrelated project, path deps like `myco-store-md`/`myco-index-redb`
# were never published, and org policy keeps every repo but tamias
# off crates.io. User-facing installs ride the tamias pack
# (`libraries/myco` → `tamias install 9aia:myco@<ver>`), so a release
# is one workspace version bump + a `myco-v<ver>` tag. The operator
# then publishes the pack itself (`tamias publish libraries/myco`).
# Binary dist stays out of scope — `myco` ships no cargo-installed
# binary (T-041 tracks prebuilt artifacts as a future option).
set -euo pipefail
# worktree-local env (gitignored) — kept for parity with the other
# org release scripts; nothing here needs secrets today.
[ -f ./.env ] && { set -a; source ./.env; set +a; }
sleep "${MYCO_RELEASE_DEBOUNCE:-300}"

git fetch origin --tags -q
last="$(git describe --tags --abbrev=0 2>/dev/null || true)"
if [ -z "$last" ]; then
    echo "no tags yet — tag the first release manually to seed the flow"; exit 0
fi
changed="$(git diff --name-only "${last}..HEAD")"

# release-worthy paths: workspace crates, the tamias pack payload,
# xtask, and the release machinery itself. Docs live on the orphan
# `docs` worktree branch — they never appear in this diff.
relevant="$(grep -E '^(crates/|libraries/|xtask/|Cargo\.(toml|lock)|scripts/release\.sh)' <<<"$changed" || true)"
if [ -z "$relevant" ]; then echo "no release-relevant changes since $last"; exit 0; fi

# every crate inherits `version.workspace` — one bump moves them all,
# and `cargo set-version --workspace` rewrites the workspace.package
# version, the workspace.dependencies reqs, and Cargo.lock together.
old="$(grep -m1 '^version = "' Cargo.toml | cut -d'"' -f2)"
cargo set-version --workspace --bump patch
ver="$(grep -m1 '^version = "' Cargo.toml | cut -d'"' -f2)"

# keep the user-facing version refs in lockstep: the tamias pack
# manifest and the two install examples.
sed -i "s/^version: \"${old}\"/version: \"${ver}\"/" libraries/myco/tamias.yaml
sed -i "s|myco@${old}|myco@${ver}|g" README.md .agents/skills/myco/SKILL.md

git commit -aqm "release: bump myco to ${ver} — tamias pack release"
git tag "myco-v${ver}"
git push -q origin HEAD:main --tags

echo "released: myco ${ver} (tag myco-v${ver})"
echo "next: tamias publish libraries/myco  # ship 9aia:myco@${ver} to the org registry"
