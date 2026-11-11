#!/usr/bin/env bash
# Manual release — run by an operator on main (bupha watch, the old
# push trigger, is retired; releases are operator-run).
# The leading sleep is the debounce window: only the last invocation
# in a quiet window releases. Commits/tags are pushed to main.
set -euo pipefail
# worktree-local secrets (gitignored); the service env file is canonical
[ -f ./.env ] && { set -a; source ./.env; set +a; }
sleep "${MYCO_RELEASE_DEBOUNCE:-300}"

git fetch origin --tags -q
last="$(git describe --tags --abbrev=0 2>/dev/null || true)"
if [ -z "$last" ]; then
    echo "no tags yet — run 'myco deploy release' to seed the flow"; exit 0
fi
changed="$(git diff --name-only "${last}..HEAD")"

touched=""
for c in myco-core myco-git myco-ops myco-shell myco-cli; do
    grep -q "^crates/$c/" <<<"$changed" && touched="$touched $c"
done
# the bin re-publishes whenever any crate changed
[ -n "$touched" ] && touched="$touched myco"
if [ -z "$touched" ]; then echo "no crate changes since $last"; exit 0; fi

order="myco-core myco-git myco-ops myco-shell myco"
published=""
tags=""
# every crate inherits `version.workspace` — one bump moves them
# all, so bump once up front rather than per touched crate (which
# inflated the shared version once per crate). The version lives
# in the root manifest, not the crate manifests.
cargo set-version -p myco-core --bump patch
ver="$(grep -m1 '^version = "' Cargo.toml | cut -d'"' -f2)"
for c in $order; do
    p="${c}"; [ "$c" = myco-cli ] && p="myco"   # cli package name
    grep -q " ${c} \|^${c} \?$\| ${c}$" <<<" $touched " || continue
    for i in 1 2 3 4 5; do
        cargo publish -p "$p" --allow-dirty && break || sleep 30
    done
    published="$published $p"
    tags="$tags $p-v$ver"
done

git commit -aqm "release: bump crate versions"
for t in $tags; do git tag "$t"; done
git push -q origin HEAD:main --tags

# binaries: local dist build (host arch) → GitHub Release.
# Other arches join when the remote-runner pair lands (TODO).
if command -v dist >/dev/null && command -v gh >/dev/null; then
    dist build --output-format=json > /tmp/dist-manifest.json 2>&1 || true
    gh release create "myco-v${ver}" --generate-notes \
        --title "myco ${ver}" 2>/dev/null || true
    for f in target/distrib/*; do
        [ -f "$f" ] && gh release upload "myco-v${ver}" "$f" --clobber
    done
fi
echo "released:${published} (tags:${tags})"
