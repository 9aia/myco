+++
id = "P-016"
title = "Ledger-selection model: formal precedence spec and naming"
status = "open"
created = "2026-11-20"
priority = "med"
links = ["T-090", "P-015"]
+++

## Overview

P-015 adds a persistent current-ledger selector, but the selection model is still prose-level: precedence order, target vocabulary, and the relationship between repo ledger / index / org designation / current are not formally specified. This proposal defines that model so P-015 (and docs, T-089) implement against a spec instead of re-deriving it.

## Requirements

- **R1 — precedence spec**: a formal, ordered resolution table covering `--dir`/`--root`, `MYCO_LEDGER`/`MYCO_LEDGER_SCOPE`, config `[current]` scope=fallback|override, cwd worktree discovery, `project_dir`, ancestor walk, org fallback, error. Each row: source, when it applies, what it can override.
- **R2 — vocabulary**: canonical definitions for *repo ledger* (a repo's own `project/` worktree), *index* (`repos.toml` registry), *universe* (lasius clones ∪ registry), *org designation* (`ledger="org"`), *current selector* (user-layer `[current]`). Naming review: is `current` the right verb/config key vs `ledger`?
- **R3 — config surface**: decide whether `[current]` lives in `myco_core::Config` with user-layer-only merge, or a dedicated user-state loader; document why repo-layer injection must be impossible.
- **R4 — failure semantics**: what happens when a persisted target goes stale (repo moved, org undesignated) — hard error vs warn-and-fallback, per scope.

## Decisions

- Feeds P-015 implementation and T-089 docs; landing this first is recommended.
