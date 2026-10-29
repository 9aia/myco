#!/usr/bin/env bash
# Build the VitePress site and publish .vitepress/dist to the `gh-pages`
# branch. Runs identically by hand (from the docs/ worktree) and under
# bupha — the watcher runs `bash scripts/deploy-docs.sh` in a detached
# worktree of the pushed docs sha, so cwd is the docs checkout root and
# `..` is NOT the main repo. Cargo work happens in MYCO_REPO (the real
# checkout); the gen-*.py scripts honor MYCO_ROOT for the same reason.
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."
DOCS="$(pwd)"
REPO="${MYCO_REPO:-$HOME/GitHub/9aia/myco}"
export MYCO_ROOT="$REPO"

echo "==> bun install"
bun install

MYCO_BIN="$(command -v myco || true)"
MYCO_BIN="${MYCO_BIN:-$HOME/.cargo/bin/myco}"
echo "==> reference/cli.md ($MYCO_BIN gen-cli-docs)"
"$MYCO_BIN" gen-cli-docs > reference/cli.md

echo "==> gen config + crate map (MYCO_ROOT=$REPO)"
python3 gen-config-docs.py
python3 gen-crate-map.py

echo "==> rustdoc → public/api (cargo doc in $REPO)"
python3 gen-api.py

echo "==> vitepress build"
bunx vitepress build

# Branch deploys still get a Jekyll pass unless .nojekyll is present.
touch .vitepress/dist/.nojekyll

echo "==> deploy .vitepress/dist → gh-pages"
bunx gh-pages -d .vitepress/dist -b gh-pages \
  -r git@github.com:9aia/myco.git \
  -m "docs: deploy $(git rev-parse --short HEAD)"

echo "done — https://9aia.com/myco/ serves origin/gh-pages"
