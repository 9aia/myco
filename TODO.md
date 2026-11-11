# TODO — myco

- [ ] T-029 presentation deck — pending: run via ~/.9aia-pres pipeline (pres kick myco or supervisor); agent writes presentation/ on branch presentation/initial, slidev build verify, push; no PR (sturn flow) — the pushed branch is the deliverable; merge is a human/sturn call; status board: ~/.9aia-pres/STATUS.md · created:2026-11-02
- [ ] T-038 pavo driver: org-index views — myco-tui's --view org-\* cross-repo boards (redb index universe) have no pavo equivalent yet; candidate myco:org view over myco-index-redb snapshots · created:2026-11-08
- [ ] T-040 retire myco ui once pavo is stable — the mutating board TUI was restored in ce91dce as a stopgap (pavo v1 forms are read-only; interactive submit is v2). When pavo forms/actions land, re-evaluate: keep myco ui as the cockpit or move mutations onto the pavo surface and drop the ratatui crate + tui feature again · pri:low · created:2026-11-08
- [ ] T-041 npm distribution phase 2 — publish @9aia/myco to a registry (npmjs scope vs GitHub Packages) and/or ship prebuilt binaries via GitHub Releases + cargo-dist (dist-workspace.toml exists, ci=none). Phase 1 (git-install shim, e858b78) needs Rust; this removes that requirement · pri:low · created:2026-11-08
- [ ] T-082 P-012 ph2: \[\[cycles]] config + check warnings + group-by-cycle · created:2026-11-09 · blocks:T-083
- [ ] T-083 P-012 ph3: timeline + stats + burnup view emits (tty+JSON, index parity) · created:2026-11-09
- [ ] T-084 P-012 ph4: done: archive stamp (write-once) + item show · created:2026-11-09 · blocks:T-083
- [ ] T-086 emit rejects  in item titles that parse accepts — unfixable emit-drift (graphopt PL-022/023); check\_item vs parser asymmetry · created:2026-11-11
