# TODO — myco

## v2 rewrite (phases — see proposals/v2.md)

- [x] Bootstrap from bupha — v0 branch, anatomy copy, rename, prune,
      worktrees, `xtask check` green
- [x] `myco-core` + `myco-store-md` — models, ID registry, state
      machine, schemas, golden fixtures (markdown-rs + insta)
- [ ] `myco-git` — `init` / `check` / `migrate` / auto-commit
- [ ] Item verbs — todo/backlog/issue/proposal/plan/adr + `item set`
- [ ] `manifest`/`schema`/`status --prompt` + `myco-index-redb` +
      board/report/org views
- [ ] Continuity — `handoff`/`resume` + sepia session hook
- [ ] Jira-parity — epics/milestones, `absorb` importers, `serve` ADR
- [ ] Skill rewrite → pilot on one repo (bupha or sepia) → iterate
- [ ] Rollout — migrate 7 `pm/` repos, normalize 12 `project/` repos,
      file `~/GitHub/9aia/v2-plan.md` into a `project/plans/`,
      release via cargo-dist
