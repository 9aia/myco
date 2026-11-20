+++
id = "P-017"
title = "Docs: ledger concepts and precedence reference"
status = "open"
created = "2026-11-20"
priority = "med"
links = ["T-089", "P-015", "P-016"]
+++

## Overview

Myco's ledger-selection concepts (repo ledger, index/universe, org designation, current selector) are diffuse and undocumented. Once P-016 fixes the formal model, the docs need to teach it: what each layer is, how they relate, and the precedence table users/agents can rely on.

## Requirements

- **R1 — concepts doc**: repo ledger vs index vs universe vs org ledger vs current selector — one mental model, one diagram.
- **R2 — precedence reference**: the formal resolution order from P-016 as a reference page, incl. `MYCO_LEDGER` env override and `--dir`/`--root`.
- **R3 — config reference**: `[current] target/scope` in user-layer `myco.toml`, including the repo-layer injection rule.
- **R4 — surfaces**: document that CLI, `myco ui`, serve/batch, and driver ops share the same discovery; `org-*` index views are a separate explicit source.

## Decisions

- Depends on P-016 for the canonical model; update generated reference (cypera) alongside hand-written docs.
