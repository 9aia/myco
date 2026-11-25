# Changelog

## [0.4.0] - 2026-11-24

## [0.3.0] - 2026-11-24

### Added

- item reorder verb + undo/redo/reorder driver actions (P-033 P-d) (`d6df565`)
- **driver:** tamias-driver-myco — pavo views replace myco-tui (`b74e0c8`)
- [sync] auto_push defaults on — opt out with auto_push = false (T-087) (`9b504a3`)
- team-collaboration hardening — actor chain, claim ownership, move resume (`9899678`)
- typed item links + clickable TUI related ids (P-013) (`93d037d`)
- index universe via lasius workspace + repo-side ledger marks (P-026) (`79f903c`)
- reopen done/cancelled items + done remove verb (`734d881`)
- add `myco reap` — bulk stale-claim reaper (T-022/PL-016 ph.4) (`6d99b20`)
- [sync] auto_push — publish the ledger branch after committed mutations (T-087) (`51d6389`)
- **driver:** bins[] cli shim passthrough on tamias-driver-myco (T-006) (`4ab158b`)
- add cancelled (wontfix) status distinct from deferred (`4021a19`)
- config loading via scarab-config (T-015) (`28e1fb7`)
- engine-as-lib + tamias driver ops pattern (kuda parity) (`0ef8b79`)
- **driver:** pavo interaction protocol — invoke, actions, menus (`0b858a2`)

### Changed

- **cli:** retire the myco compat bin — engine e2e via tamias-driver-myco run (`c1024c0`)
- retire myco-git — git access via shared lasius-vcs port (P-026) (`8f6bc91`)
- **myco-driver:** adopt pavo-driver-kit (`29871a6`)

### Fixed

- **install:** pavo-protocol via git — sibling path dep broke fresh-clone installs (`61a5993`)
