+++
id = "PL-002"
title = "pm-parity scheduling fields cycles roadmap stats"
status = "open"
created = "2026-11-09"
priority = "medium"
+++

Executes P-012 (accepted 2026-11-09). Four independently shippable phases; each lands as its own commit series with tests per the proposal's test plan.

## Phases

- [ ] Phase 1 — item fields: `due:`/`est:`/`cycle:` tokens + frontmatter on `Item`, `item set`/add flags, `myco schema`, store-md goldens
- [ ] Phase 2 — `[[cycles]]` config: parse/validate, `check` warnings for unknown cycle names, group-by-cycle axis
- [ ] Phase 3 — emits: `timeline` + `stats` + `burnup` view emits (tty ASCII + JSON), index-source parity, docs
- [ ] Phase 4 — `done:` archive stamp (write-once), `item show` surface — burnup's data source

Edges: 1 → 2 → 3; 4 → 3 (burnup needs stamps). Phase 4 can start any time.
