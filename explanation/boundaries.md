---
url: /myco/explanation/boundaries.md
---
# Boundaries

myco owns the ledger of intent (tasks, issues, proposals, plans,
ADRs, handoffs). Neighbors own their own memory:

* **sturn** — ephemeral coordination (live claims/ops/topics)
* **kuda** — code memory; myco indexes work items, not code
* **peregrin** — orchestration; it embeds the ledger through
  `myco serve` (JSON-RPC 2.0 on stdio — `adrs/0001`), reachable
  one-shot via `myco batch`
* **bupha** — CI gate (was: `myco check` in pipelines,
  `Item:` trailers); bupha is the CI repo again — the org gate is
  mid-migration back from the peregrin `ci-gate` campaign (PL-013)
* **mycena** — surfaces TODO/FIXME in code → `myco absorb`
* **sepia** — session lifecycle → `myco handoff` / `myco resume`

Anything needing a daemon, a multi-machine view, or arbitration
between actors belongs to peregrin, not here.
