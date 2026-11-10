#!/usr/bin/env python3
"""cargo doc → docs/public/api + styled index. Resolves the real
target dir via `cargo metadata` (respects a configured
`target-dir`); wraps gen-api-index.py for the index page."""
import json, os, pathlib, shutil, subprocess

DOCS = pathlib.Path(__file__).resolve().parent
# The main checkout — docs/ normally mirrors it as a worktree (so
# DOCS.parent works); in CI the docs sha runs detached and
# MYCO_ROOT points at the real repo.
ROOT = pathlib.Path(os.environ.get("MYCO_ROOT") or DOCS.parent)

# Isolate the doc build in a repo-local target dir. The user-level
# `target-dir` (e.g. ~/.cargo/shared-target) is shared across repos
# and shas and has served stale rmeta — a poisoned `myco-core` unit
# made `cargo doc --workspace` fail with a phantom type error until
# `cargo clean -p` cleared it. A dedicated dir under the repo's own
# (gitignored) target/ stays warm across gate runs and can't be
# poisoned by artifacts from other checkouts. An explicit
# CARGO_TARGET_DIR still wins.
os.environ.setdefault(
    "CARGO_TARGET_DIR", str(ROOT / "target" / "docs-api"))


def run(*args, capture=False):
    return subprocess.run(
        list(args), cwd=ROOT, check=True,
        capture_output=capture, text=True)


run("cargo", "doc", "--workspace", "--no-deps")
meta = json.loads(run("cargo", "metadata", "--no-deps",
                      "--format-version", "1", capture=True).stdout)
doc = pathlib.Path(meta["target_directory"]) / "doc"

api = DOCS / "public" / "api"
api.mkdir(parents=True, exist_ok=True)
for c in api.iterdir():
    shutil.rmtree(c) if c.is_dir() else c.unlink()
for c in doc.iterdir():
    if c.is_dir():
        shutil.copytree(c, api / c.name)
    else:
        shutil.copy2(c, api / c.name)

run("python3", str(DOCS / "gen-api-index.py"), str(api / "index.html"))
