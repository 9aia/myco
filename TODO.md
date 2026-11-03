# TODO — myco

- [ ] T-029 presentation deck — pending: run via ~/.9aia-pres pipeline (pres kick myco or supervisor); agent writes presentation/ on branch presentation/initial, slidev build verify, push; no PR (sturn flow) — the pushed branch is the deliverable; merge is a human/sturn call; status board: ~/.9aia-pres/STATUS.md · created:2026-11-02
- [ ] T-013 migrate project-mode should accept freeform '- ' bullets (pm-style lists) — currently errors; needed for pm→project flows that can't use pm-mode's main-branch commit · status:in-progress · owner:devin · created:2026-10-29
  - [ ] T-014 verify nested children parse/emit round-trip on real ledgers (cheloni, thal) · created:2026-10-29
- [ ] T-025 sepia: session teardown calls myco interrupt — sepia owns lifecycle, knows its session id; --session sepia:\<id> tags items at claim/handoff so teardown interrupts exactly its own (pattern in continuity.md) · status:in-progress · owner:devin · created:2026-10-31
- [ ] T-011 myco commit identity — repo config vs actor-tagged trailers
