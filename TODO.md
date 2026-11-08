# TODO — myco

- [ ] T-029 presentation deck — pending: run via ~/.9aia-pres pipeline (pres kick myco or supervisor); agent writes presentation/ on branch presentation/initial, slidev build verify, push; no PR (sturn flow) — the pushed branch is the deliverable; merge is a human/sturn call; status board: ~/.9aia-pres/STATUS.md · created:2026-11-02
- [ ] T-038 pavo driver: org-index views — myco-tui's --view org-\* cross-repo boards (redb index universe) have no pavo equivalent yet; candidate myco:org view over myco-index-redb snapshots · created:2026-11-08
- [ ] T-040 retire myco ui once pavo is stable — the mutating board TUI was restored in ce91dce as a stopgap (pavo v1 forms are read-only; interactive submit is v2). When pavo forms/actions land, re-evaluate: keep myco ui as the cockpit or move mutations onto the pavo surface and drop the ratatui crate + tui feature again · pri:low · created:2026-11-08
