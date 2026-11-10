+++
id = "I-026"
title = "shared-target stale rmeta — isolate CARGO_TARGET_DIR in gate scripts"
status = "done"
created = "2026-10-31"
owner = "devin"
priority = "med"
+++

~/.cargo/config.toml routes every repo's cargo into one shared target-dir. Stale fingerprints linked units built from older source — I-024's phantom clippy errors were this, and eqsat/host ci.sh carry the same risk. Docs already isolates (51abb9c, target/docs-api). Apply the same CARGO_TARGET_DIR isolation to xtask check and ci.sh gates; alternatively drop the global shared target-dir.
