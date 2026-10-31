+++
id = "I-023"
title = "audit lows: config/tooling"
status = "done"
created = "2026-10-31"
priority = "low"
labels = ["audit"]
+++

TomlSource.path is dead — no --config flag ever populates it; missing explicit/$MYCO_CONFIG path gives generic error; XDG_CONFIG_HOME ignored (config-toml/lib.rs:15-28). myco-shell is raw bash -c — safe today (kinds validated) but add argv-taking API or doc-warning for future callers (myco-shell/lib.rs). xtask site ignores bun/npm install failure → confusing build errors (xtask/main.rs:120). config-toml data fallback ./.config/myco in cwd when HOME unset (mirror of fixed index-redb case, lib.rs:23-26).

## Resolution

fixed e86db8d — XDG_CONFIG_HOME, explicit-path errors, shell/pager doc-warnings, xtask install check
