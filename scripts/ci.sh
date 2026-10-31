#!/usr/bin/env bash
# bupha gate for main — runs in a detached worktree of the pushed sha.
# check = fmt + clippy -D warnings; test = the whole suite.
set -euo pipefail

# The bupha watcher unit runs with the minimal systemd PATH
# (/usr/local/bin:/usr/bin) whenever it starts before the session env is
# imported — pull the user tool dirs in ourselves so cargo resolves like
# it does in a login shell.
export PATH="$HOME/.cargo/bin:$HOME/.local/bin:$PATH"

cargo xtask check
cargo xtask test
