# Changelog

## [0.4.0] - 2026-11-24

## [0.3.0] - 2026-11-24

### Added

- workspace selector — [workspace] repos globs over the universe (P-016) (`f18037f`)
- item scheduling fields due/est/cycle (T-081, P-012 ph1) (`df1cee2`)
- verb-fronted views — read verbs ride the eval engine (T-027) (`d4db930`)
- link index (P-005) + verb registry & config verbs (P-007) (`c570493`)
- org-as-view — ViewSpec.source + GroupAxis::Repo (T-035) (`bcdb1ee`)
- **driver:** tamias-driver-myco — pavo views replace myco-tui (`b74e0c8`)
- myco-ops split + `myco ui` TUI + paged output (`a5d9e3a`)
- **serve:** myco serve — JSON-RPC 2.0 verb surface on stdio (ADR-0001) (`9b76050`)
- [sync] auto_push defaults on — opt out with auto_push = false (T-087) (`9b504a3`)
- team-collaboration hardening — actor chain, claim ownership, move resume (`9899678`)
- **sync:** adopt remote ledgers, sync verb, behind-remote guard (`8ad39b0`)
- multi-level undo/redo via journaled history stack (`88d65cf`)
- **tui:** mouse — wheel scroll, click-select, click tabs (T-020) (`83d152d`)
- default_view + ViewFormat::Repos (`8133794`)
- collapse board/epic/milestone into myco view + ConfigLoader seam (T-034) (`8087795`)
- index universe via lasius workspace + repo-side ledger marks (P-026) (`79f903c`)
- declarative views — named [views] specs in config, myco view verb, ops presets (P-006) (`6abd9d2`)
- next as a builtin view — the verb resolves it (T-034 tail) (`5b18108`)
- Actor: commit trailers + session-tagged claims (T-011, T-025) (`5a925e1`)
- [sync] auto_push — publish the ledger branch after committed mutations (T-087) (`51d6389`)
- add cancelled (wontfix) status distinct from deferred (`4021a19`)
- current-ledger selector — [current], MYCO_LEDGER, .myco.toml pointers (P-015) (`328714d`)
- config loading via scarab-config (T-015) (`28e1fb7`)
- myco redo — restore the last undone commit (`15aac1c`)
- **index:** org-wide ledger — one project/ for the whole org (`123cf89`)
- undo, tty confirmations, DONE.md archive, richer list filters (`102e0ca`)
- related items on item show + TUI section jumps + gate target isolation (`009859a`)

### Changed

- **config:** route myco.toml probes through scarab-config-file (P-017) (`9431a30`)
- retire myco-git — git access via shared lasius-vcs port (P-026) (`8f6bc91`)
- pure domain up to myco-core — ops is now the I/O shell (`857f9d3`)
- verbs generic over Store port + item_path on the trait (`0962769`)

### Fixed

- audit lows — config, cli/ops robustness (I-022, I-023) (`e86db8d`)
- check flags dangling supersedes/superseded_by (I-028) (`b62374c`)
- store/file-spec edge cases (I-021) + kind-table & org-tui wip (`abbfdb5`)
- **cli:** json parity — prompt --json, board interrupted, org paths; section-preserving item set (`9028c2d`)
- **ops:** serialize mutations via worktree lock; harden index registry (`899431e`)
- scope org-ledger fallback to the anchor's lasius workspace (`2fcff77`)
- **git:** scope commits to worktree, propagate git errors, guard update-ref (`1cd2d80`)
- **core:** TaskLoc APIs return Option — unbreaks tui build (I-012) (`1b656b9`)
- **ops,store-md:** don't attach foreign project dirs; heading-safe emit — I-010, I-011, I-017 (`10bacb7`)
