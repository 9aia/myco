# Changelog

## [0.4.0] - 2026-11-24

## [0.3.0] - 2026-11-24

### Added

- jira-parity — epics/milestones, absorb importers, serve ADR (`fecd899`)
- myco-core + myco-store-md — domain model, id registry, state machine, markdown store (`f10d03a`)
- item verbs — todo/backlog/issue/proposal/plan/adr + item set (`e152692`)
- check prose-ref validation + hardened jira/github importers (`dfbbdc9`)
- item scheduling fields due/est/cycle (T-081, P-012 ph1) (`df1cee2`)
- nested subtasks — children in item model, nested emit/parse, --parent (`d5f98e3`)
- typed item links + clickable TUI related ids (P-013) (`93d037d`)
- add cancelled (wontfix) status distinct from deferred (`4021a19`)
- continuity — handoff/resume + handoffs/ store (`190406d`)
- undo, tty confirmations, DONE.md archive, richer list filters (`102e0ca`)
- lifecycle verbs — init, check, migrate, commit, auto-commit plumbing (`0baf449`)

### Changed

- verbs generic over Store port + item_path on the trait (`0962769`)

### Fixed

- store/file-spec edge cases (I-021) + kind-table & org-tui wip (`abbfdb5`)
- **cli:** json parity — prompt --json, board interrupted, org paths; section-preserving item set (`9028c2d`)
- **store-md:** frontmatter priority emits pri: tokens, not serde names (I-032) (`5c96eff`)
- **check:** heal `---` YAML frontmatter in item files (`4818d82`)
- id preservation — migrate honors filename numbers, check repairs title mismatches (`1749528`)
- **ops,store-md:** don't attach foreign project dirs; heading-safe emit — I-010, I-011, I-017 (`10bacb7`)
