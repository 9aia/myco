#!/usr/bin/env bash
# bupha gate for main — runs in a detached worktree of the pushed sha.
# check = fmt + clippy -D warnings; test = the whole suite;
# coverage = the line-coverage floor in scripts/coverage-floor.
set -euo pipefail

# Script location, resolved — the gate runs from a detached worktree,
# so $PWD is the tree but the floor file travels with the script.
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# The bupha watcher unit runs with the minimal systemd PATH
# (/usr/local/bin:/usr/bin) whenever it starts before the session env is
# imported — pull the user tool dirs in ourselves so cargo resolves like
# it does in a login shell.
export PATH="$HOME/.cargo/bin:$HOME/.local/bin:$PATH"

# Isolated target dir (I-026): the default `target/` is shared with
# agent/interactive builds — a killed or concurrent run can leave
# stale rmeta and false-fail the gate. `target/gate` is job-scoped
# and stays under the already-gitignored `target/`. `cancel_running`
# kills mid-build, so a dedicated dir keeps partial artifacts from
# leaking into anyone else's builds.
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$PWD/target/gate}"

cargo xtask check
cargo xtask test

# cargo-llvm-cov looks for `llvm-cov`/`llvm-profdata` inside a rustup
# sysroot. A distro Rust (Arch's `rust`, Debian's `rustc`) ships none,
# so point it at the system LLVM instead — it works whenever the LLVM
# major matches rustc's (`rustc -vV`). Arch: `pacman -S llvm`.
if [ -z "${LLVM_COV:-}" ] && command -v llvm-cov >/dev/null 2>&1; then
  export LLVM_COV="$(command -v llvm-cov)"
  if command -v llvm-profdata >/dev/null 2>&1; then
    export LLVM_PROFDATA="$(command -v llvm-profdata)"
  fi
fi

# Coverage floor — P-001's quality bar, enforced as "never regress"
# rather than a distant aspiration. `xtask coverage` passes args
# through to llvm-cov, so the floor is one number to raise over
# time; it is not 100 yet and pretending otherwise makes the gate
# a lie (the same failure mode ADR-0002 fixed for `check`).
COVERAGE_FLOOR="${COVERAGE_FLOOR:-$(cat "$ROOT/scripts/coverage-floor" 2>/dev/null || echo 0)}"
if [ "$COVERAGE_FLOOR" != "0" ]; then
  cargo xtask coverage --fail-under-lines "$COVERAGE_FLOOR"
else
  echo "coverage: floor unset (scripts/coverage-floor) — skipping" >&2
fi
