#!/usr/bin/env bash
# ci-gate campaign gate for main — runs in a detached worktree of the
# pushed sha (bupha watch is retired; the peregrin ci-gate campaign
# is the gate).
# check = fmt + clippy -D warnings; test = the whole suite;
# coverage = the line-coverage floor in scripts/coverage-floor.
set -euo pipefail

export RUSTC_WRAPPER="${RUSTC_WRAPPER:-sccache}"
export SCCACHE_CONF="${SCCACHE_CONF:-file://$HOME/.cache/sccache}"

# Script location, resolved — the gate runs from a detached worktree,
# so $PWD is the tree but the floor file travels with the script.
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# The gate may run with the minimal systemd PATH
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

# Docs contract — cypera check + docs/ drift gate. Materializes a
# detached docs-branch worktree when the run tree lacks one (bupha
# gate runs), so it is hermetic in CI.
bash scripts/docs-check.sh

# --- mimus standard: org conformance leg (9aia-standard pack) ------
# Specs + runner ship in the 9aia-standard tamias pack; `tamias
# install` is a cache hit after the first fetch. Missing tooling
# skips with a warning; fail-severity spec violations fail the gate.
export PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH"
if command -v tamias >/dev/null 2>&1 && command -v mimus >/dev/null 2>&1; then
  _std_label="$(tamias install --no-bins 'git:github.com/9aia/workspace//libraries/9aia-standard' 2>/dev/null | sed -n 's/^installed \([^ ]*\).*/\1/p' | tail -1)"
  _std_dir=""
  [ -n "$_std_label" ] && _std_dir="$(tamias info "$_std_label" --json 2>/dev/null | python3 -c 'import json,sys; print(json.load(sys.stdin).get("dir",""))' 2>/dev/null || true)"
  # Offline rerun fallback: whatever single install already exists.
  [ -z "$_std_dir" ] && _std_dir="$(tamias info 9aia-standard --json 2>/dev/null | python3 -c 'import json,sys; print(json.load(sys.stdin).get("dir",""))' 2>/dev/null || true)"
  if [ -n "$_std_dir" ] && [ -f "$_std_dir/ci-standard.sh" ]; then
    bash "$_std_dir/ci-standard.sh"
  else
    echo "ci-standard: 9aia-standard pack unavailable — leg skipped" >&2
  fi
else
  echo "ci-standard: tamias/mimus not on PATH — leg skipped" >&2
fi
