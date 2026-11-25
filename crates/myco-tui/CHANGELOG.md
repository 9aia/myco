# Changelog

## [0.5.0] - 2026-11-24

### Added

- **view:** views/ directory convention — file-per-view loading (`aff532b`)
- **query:** negation, OR alternatives, relative due dates (P-058 phase 3) (`04a641e`)

## [0.4.0] - 2026-11-24

### Added

- **view:** query-language filters — --q/--query, TUI /, --view, config sugar (P-058) (`a6f40b6`)

## [0.3.0] - 2026-11-24

### Added

- **ui:** filesystem-watcher auto-refresh (`f8cab31`)
- item scheduling fields due/est/cycle (T-081, P-012 ph1) (`df1cee2`)
- **ui:** restore myco ui — the mutating board TUI is back (`ce91dce`)
- link index (P-005) + verb registry & config verbs (P-007) (`c570493`)
- TUI help overlay + adaptive status-bar hints (`bb4faf6`)
- **driver:** tamias-driver-myco — pavo views replace myco-tui (`b74e0c8`)
- **ui:** content-hashed auto-refresh + stack navigation (`ac9c703`)
- myco-ops split + `myco ui` TUI + paged output (`a5d9e3a`)
- typed item links + clickable TUI related ids (P-013) (`93d037d`)
- multi-level undo/redo via journaled history stack (`88d65cf`)
- **tui:** mouse — wheel scroll, click-select, click tabs (T-020) (`83d152d`)
- **ui:** error toasts stick until next input (`83c91b2`)
- **ui:** tab/shift-tab cycle boards with wrap (`8292a9d`)
- collapse board/epic/milestone into myco view + ConfigLoader seam (T-034) (`8087795`)
- declarative views — named [views] specs in config, myco view verb, ops presets (P-006) (`6abd9d2`)
- **ui:** status messages are toasts — expire after 4s (`64be94a`)
- Actor: commit trailers + session-tagged claims (T-011, T-025) (`5a925e1`)
- add cancelled (wontfix) status distinct from deferred (`4021a19`)
- **ui:** auto-refresh on external writes + full test coverage (`38b6571`)
- current-ledger selector — [current], MYCO_LEDGER, .myco.toml pointers (P-015) (`328714d`)
- **tui:** ITEMS board, / filter, session restore (T-020) (`321aa29`)
- config loading via scarab-config (T-015) (`28e1fb7`)
- **tui:** ':' command mode — q/w/e/go/set (finishes fc7050f) (`249f43e`)
- responsive TUI layout for narrow/short terminals (`167a111`)
- myco redo — restore the last undone commit (`15aac1c`)
- undo, tty confirmations, DONE.md archive, richer list filters (`102e0ca`)
- related items on item show + TUI section jumps + gate target isolation (`009859a`)

### Changed

- retire myco-git — git access via shared lasius-vcs port (P-026) (`8f6bc91`)
- pure domain up to myco-core — ops is now the I/O shell (`857f9d3`)
- verbs generic over Store port + item_path on the trait (`0962769`)

### Fixed

- audit drift — schema test, manifest, stale docs (`fc7050f`)
- **ui:** cache board fingerprint — stat-only steady-state polls (`c16e4c9`)
- store/file-spec edge cases (I-021) + kind-table & org-tui wip (`abbfdb5`)
- wire myco-tui bin to the landed ratatui front-end (`91baede`)
- **tui:** rename ITEMS tab → FILES; error on stale take() (`8a33074`)
- **ops:** serialize mutations via worktree lock; harden index registry (`899431e`)
- **tui:** surface mutation errors, per-mutation lock, terminal restore (`6574832`)
- **ui:** undo offers force-confirm on published tip; done prompt reflects DONE.md archive (`586e8e5`)
