+++
id = "I-032"
title = 'frontmatter priority emits "medium" — canonical is CLI token "med"'
status = "done"
created = "2026-11-10"
owner = "devin"
priority = "med"
+++

itemfile emit serializes priority via serde full names (`medium`/`critical`), but the CLI/vocabulary is the `pri:` token enum (`low|med|high|crit`). Frontmatter and the CLI disagree; kuda patched its domain schema to accept both spellings. Fix: emit writes the token; read stays lenient (`medium`/`critical` still parse); check --fix normalizes drift. Then tighten kuda data/domains/myco.toml back.

## Resolution

emit writes priority.token() (med/crit) — crates/myco-store-md itemfile.rs; read stays lenient via Priority::from_str so medium/critical normalize on any rewrite; check --fix repaired 33 ledger files; kuda data/domains/myco.toml tightened back (kuda I-001)
