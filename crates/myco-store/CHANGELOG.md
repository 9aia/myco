# Changelog

## [0.3.0] - 2026-11-24

### Added

- myco-core + myco-store-md — domain model, id registry, state machine, markdown store (`f10d03a`)
- item verbs — todo/backlog/issue/proposal/plan/adr + item set (`e152692`)
- continuity — handoff/resume + handoffs/ store (`190406d`)
- undo, tty confirmations, DONE.md archive, richer list filters (`102e0ca`)

### Changed

- verbs generic over Store port + item_path on the trait (`0962769`)

### Fixed

- audit drift — schema test, manifest, stale docs (`fc7050f`)
- store/file-spec edge cases (I-021) + kind-table & org-tui wip (`abbfdb5`)
