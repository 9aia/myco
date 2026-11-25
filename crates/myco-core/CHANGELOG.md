# Changelog

## [0.5.0] - 2026-11-24

### Added

- **query:** negation, OR alternatives, relative due dates (P-058 phase 3) (`04a641e`)

## [0.4.0] - 2026-11-24

### Added

- **view:** query-language filters — --q/--query, TUI /, --view, config sugar (P-058) (`a6f40b6`)

## [0.3.0] - 2026-11-24

### Added

- jira-parity — epics/milestones, absorb importers, serve ADR (`fecd899`)
- workspace selector — [workspace] repos globs over the universe (P-016) (`f18037f`)
- myco-core + myco-store-md — domain model, id registry, state machine, markdown store (`f10d03a`)
- item verbs — todo/backlog/issue/proposal/plan/adr + item set (`e152692`)
- item scheduling fields due/est/cycle (T-081, P-012 ph1) (`df1cee2`)
- nested subtasks — children in item model, nested emit/parse, --parent (`d5f98e3`)
- link index (P-005) + verb registry & config verbs (P-007) (`c570493`)
- org-as-view — ViewSpec.source + GroupAxis::Repo (T-035) (`bcdb1ee`)
- `myco interrupt` + interrupted visibility + stale claims (`b6a5b7f`)
- [sync] auto_push defaults on — opt out with auto_push = false (T-087) (`9b504a3`)
- team-collaboration hardening — actor chain, claim ownership, move resume (`9899678`)
- typed item links + clickable TUI related ids (P-013) (`93d037d`)
- default_view + ViewFormat::Repos (`8133794`)
- collapse board/epic/milestone into myco view + ConfigLoader seam (T-034) (`8087795`)
- index universe via lasius workspace + repo-side ledger marks (P-026) (`79f903c`)
- reopen done/cancelled items + done remove verb (`734d881`)
- declarative views — named [views] specs in config, myco view verb, ops presets (P-006) (`6abd9d2`)
- [sync] auto_push — publish the ledger branch after committed mutations (T-087) (`51d6389`)
- add cancelled (wontfix) status distinct from deferred (`4021a19`)
- current-ledger selector — [current], MYCO_LEDGER, .myco.toml pointers (P-015) (`328714d`)
- config loading via scarab-config (T-015) (`28e1fb7`)
- continuity — handoff/resume + handoffs/ store (`190406d`)
- undo, tty confirmations, DONE.md archive, richer list filters (`102e0ca`)

### Changed

- bootstrap v2 from bupha anatomy — Rust workspace, pruned to PM infra (`e2b70bf`)
- pure domain up to myco-core — ops is now the I/O shell (`857f9d3`)

### Fixed

- audit drift — schema test, manifest, stale docs (`fc7050f`)
- store/file-spec edge cases (I-021) + kind-table & org-tui wip (`abbfdb5`)
- **install:** pavo-protocol via git — sibling path dep broke fresh-clone installs (`61a5993`)
- **store-md:** frontmatter priority emits pri: tokens, not serde names (I-032) (`5c96eff`)
- **core:** TaskLoc APIs return Option — unbreaks tui build (I-012) (`1b656b9`)
- **ops,store-md:** don't attach foreign project dirs; heading-safe emit — I-010, I-011, I-017 (`10bacb7`)
