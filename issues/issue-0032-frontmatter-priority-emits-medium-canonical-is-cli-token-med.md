+++
id = "I-032"
title = 'frontmatter priority emits "medium" — canonical is CLI token "med"'
status = "open"
created = "2026-11-10"
owner = "devin"
priority = "medium"
+++

itemfile emit serializes priority via serde full names (`medium`/`critical`), but the CLI/vocabulary is the `pri:` token enum (`low|med|high|crit`). Frontmatter and the CLI disagree; kuda patched its domain schema to accept both spellings. Fix: emit writes the token; read stays lenient (`medium`/`critical` still parse); check --fix normalizes drift. Then tighten kuda data/domains/myco.toml back.
