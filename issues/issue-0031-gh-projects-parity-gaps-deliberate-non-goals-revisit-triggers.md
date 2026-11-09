+++
id = "I-031"
title = "GH-Projects parity gaps — deliberate non-goals, revisit triggers"
status = "open"
created = "2026-11-09"
priority = "low"
labels = ["parity"]
+++

## Context

P-012 closes the *planning-surface* gap vs GitHub Projects (due dates, estimates, cycles, roadmap, stats, burnup). This issue records what remains different **on purpose** — myco is agent work-memory synced by git, not a GitHub competitor — so future "are we missing X?" audits don't re-litigate them.

## Deliberate non-goals (revisit only if the premise changes)

- **Automations** — GH rules (auto-add, auto-transition, field updates) fire themselves; myco verbs are explicit. A rules engine would be a daemon-shaped feature → belongs to peregrin if ever.
- **PR/issue integration** — GH Projects *is* issues+PRs; myco is standalone markdown. No PR state, reviewers, or CI checks on items. If linking is wanted, it's an integration concern (mycena/bupha territory), not ledger schema.
- **Teams/notifications** — `owner:` is an actor string, not an account; no @mentions, notifications, reactions, presence. Sync is git pull — async by design.
- **Arbitrary custom fields** — fixed metadata set + `[[kinds]]` config + labels. GH has typed custom fields; myco's answer is kinds+labels+views, not a field registry. Revisit if real usage demands it.
- **50k-item scale / mobile** — no such targets; index (redb) is the scale answer for org-wide queries.

## Where myco is already ahead

Proposals/ADRs/plans/handoffs as first-class kinds · agent-first (`--json`, manifest, `status --prompt`) · git-native audit + journaled undo/redo · org-wide ledger across repos · TUI cockpit.

## Revisit triggers

- Users ask for rule-driven status changes → automations proposal (peregrin?).
- Ledger fields stabilize → maybe typed custom fields via `[[kinds]]` extension.
- Multi-human teams adopt myco → reassess owner:/notifications entirely.
