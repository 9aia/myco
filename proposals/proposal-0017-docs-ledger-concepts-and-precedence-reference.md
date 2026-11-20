+++
id = "P-017"
title = "Docs: ledger concepts and precedence reference"
status = "open"
created = "2026-11-20"
priority = "med"
links = ["T-089", "P-015", "P-016"]
+++

## Overview

Myco's ledger-selection concepts (repo ledger, index/universe, org designation, current selector, directory pointer, workspace) are diffuse and undocumented. Once P-016 fixes the formal model, the docs need to teach it: what each layer is, how they relate, and the precedence table users/agents can rely on.

## Requirements

- **R1 — concepts doc**: the scenario model from P-016 — repo auto-detect, `.myco.toml` directory pointer, `[current]` global pin (`always`/`backup`), per-invocation overrides, workspace (multi-repo globs), org fallback — one mental model, one diagram.
- **R2 — precedence reference**: the formal 11-row resolution order from P-016 as a reference page, incl. `MYCO_LEDGER`/`MYCO_LEDGER_SCOPE`, `--dir`/`--root`, and the `MYCO_LEDGER=repo` escape hatch.
- **R3 — config reference**: `[current] ledger/mode` + `[workspace] repos` in user-layer `myco.toml`, `.myco.toml` pointer files (non-repo dirs only, managed via `current use|clear --here`), target vocabulary (`repo`/`org`/name/path/`name:`), repo-layer injection rule.
- **R4 — surfaces**: document that CLI, `myco ui`, serve/batch, and driver ops share the same discovery; `org-*` index views and workspace-scoped operations are separate explicit sources.

## Decisions

- Depends on P-016 for the canonical model; update generated reference (cypera) alongside hand-written docs.
