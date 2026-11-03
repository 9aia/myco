#!/usr/bin/env bash
# bupha gate for main — runs in a detached worktree of the pushed sha.
# check = fmt + clippy -D warnings; test = the whole suite.
set -euo pipefail

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
