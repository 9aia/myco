# TODO — myco

- [ ] T-029 presentation deck — pending: run via ~/.9aia-pres pipeline (pres kick myco or supervisor); agent writes presentation/ on branch presentation/initial, slidev build verify, push; no PR (sturn flow) — the pushed branch is the deliverable; merge is a human/sturn call; status board: ~/.9aia-pres/STATUS.md · created:2026-11-02
- [ ] T-030 spec: managed AGENTS.md block convention — markers \<!-- tool:agent-awareness -->, per-tool ownership, idempotent rewrite preserving prose, multi-tool coexistence (sturn/kuda/bupha/mimus blocks now deployed), check --fix semantics. myco's init is the reference impl; extract the spec so other tools implement identically · created:2026-11-02
- [ ] T-033 inject impl ConfigSource into discover/named\_views — ops still constructs TomlSource directly (last impl dep in myco-ops) · pri:low · created:2026-11-02
