# TODO — myco

- [ ] T-038 pavo driver: org-index views — myco-tui's --view org-\* cross-repo boards (redb index universe) have no pavo equivalent yet; candidate myco:org view over myco-index-redb snapshots · created:2026-11-08
- [ ] T-040 retire myco ui once pavo is stable — the mutating board TUI was restored in ce91dce as a stopgap (pavo v1 forms are read-only; interactive submit is v2). When pavo forms/actions land, re-evaluate: keep myco ui as the cockpit or move mutations onto the pavo surface and drop the ratatui crate + tui feature again · pri:low · created:2026-11-08
- [ ] T-041 npm distribution phase 2 — publish @9aia/myco to a registry (npmjs scope vs GitHub Packages) and/or ship prebuilt binaries via GitHub Releases + cargo-dist (dist-workspace.toml exists, ci=none). Phase 1 (git-install shim, e858b78) needs Rust; this removes that requirement · pri:low · created:2026-11-08
- [ ] T-086 emit rejects middot in item titles that parse accepts — unfixable emit-drift (graphopt PL-022/023); check\_item vs parser asymmetry · created:2026-11-11
