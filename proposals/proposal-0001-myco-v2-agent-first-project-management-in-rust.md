+++
id = "P-001"
title = "myco v2 — agent-first project management, in Rust"
status = "open"
priority = "medium"
+++

# myco v2 — agent-first project management, in Rust

Status: **proposal v2** (amendments folded). Replaces skill-only myco
with a bupha-anatomy Rust repo: a `myco` CLI owns every deterministic
write to `project/`; the skill teaches agents the verbs, not file
formats. Ambition: full PM replacement — drop `pm/`/`project/` drift,
GitHub Projects, and Jira for 9aia and personal work.

## Position in the 9aia stack (boundary)

myco = **work memory** — the ledger of intent (tasks, issues,
proposals, plans, ADRs, handoffs). Neighbors own:

- **sturn** — ephemeral coordination: live claims/ops/topics.
  `claim T-012` is sturn's; myco items just carry `owner:` refs.
- **kuda** — code memory. myco indexes *work items*, not code;
  shared index/cache pattern only.
- **peregrin** — orchestration: campaign nodes call `myco` verbs as
  workflow steps; optional `myco serve` (ndjson) is a later ADR.
- **bupha** — CI gate: `myco check` in pipelines (docs-as-tests for
  PM files); commit trailers `Item: T-012`.
- **mycena** — surfaces TODO/FIXME comments in code → `myco absorb`
  promotes them to tracked items.
- **sepia** — session lifecycle: session end → `myco handoff`;
  next session → `myco resume`.

## Locked decisions

- **Hybrid store** — markdown in `project/` is canonical and
  human-editable; the index (redb) is a **disposable derived cache**
  (`myco index rebuild`), powering queries + org views. `myco check`
  repairs drift between file and index.
- **Numbered slugs, one registry** — per-type counters in a single
  ID namespace: `T-012` tasks, `I-003` issues, `P-002` proposals,
  `ADR-0004`, `PL-001` plans.
- **State machine in `myco-core`** — owned transitions
  (open→claimed→in-progress→blocked→done / deferred); invalid
  states are unrepresentable.
- **Actor IDs** — `owner:`/`assignee:` take `human:luis`, `devin`,
  `claude`, … first-class multi-actor, aligned with peregrin
  arbitration.
- **PM-only scope** — `myco` manages `project/`; `docs/` worktrees
  stay repo tooling (bupha pattern).
- **Auto-commit per verb** — each mutation is one atomic commit on
  the `project` branch (`--no-commit` opt-out). Agents never touch
  git directly; history is the audit trail.
- **Agent-first wire** — `--json` everywhere + self-describing
  surface (`manifest`, `schema`) so the skill shrinks to a trigger.

## Crate map (bupha ports/adapters)

| Crate | Role |
| --- | --- |
| `myco-core` | pure domain: item models, ID registry, state machine, deps — no I/O |
| `myco-store` | store port |
| `myco-store-md` | markdown adapter — strict parse/emit, round-trip stable; **the file spec** |
| `myco-index` + `myco-index-redb` | derived index port + impl — `status`/`board`/`report`/org views |
| `myco-git` | orphan `project` branch, worktree, auto-commit |
| `myco-template` | scaffolds + document templates |
| `myco-config` + `myco-config-toml` | `myco.toml` per repo |
| `myco-cli` | the `myco` binary (clap, `--json` everywhere) |
| `myco-testkit` | golden fixtures + conformance — frozen md round-trips |
| `xtask` | check / test / coverage / install / site |

## CLI surface

```bash
# discovery (agent self-service)
myco manifest             # capabilities + protocol version
myco schema               # the file/state spec, machine-readable

# lifecycle
myco init                 # orphan branch + worktree + .gitignore + scaffold
myco check [--fix]        # validate schemas + state machine, repair drift
myco migrate              # pm/ → project/, legacy → v2 schema
myco absorb               # import TODO/FIXME comments, pm/, Jira/GH export
myco index rebuild        # rebuild derived index

# items — all --json, auto-commit
myco todo add|done|defer|list
myco backlog add|promote|list
myco issue new|list|resolve|archive|comment
myco proposal new|list|status
myco plan new|list
myco adr new|list|supersede
myco item set <id> --status|--priority|--assignee|--label|--blocks

# continuity (the agent-first core)
myco handoff <id> --note|--session  # attach resumable state at session end
myco resume <id>                    # minimal context block to restart work
myco status [--prompt]              # cheap digest; --prompt = token-sized

# views + org
myco board                # kanban render (md or --json)
myco report               # aging/velocity summaries
myco index add <repo>     # register repos → org query
myco org status|board     # cross-repo views via the index

# later (Jira-parity, phased)
myco epic|milestone …     # grouping views over item metadata
myco serve                # ndjson JSON-RPC for peregrin embedding [ADR]
```

## Dependencies — lib-first, no hand-rolled code

Convention-consistent picks (what bupha/sepia/sturn already use):

- **markdown**: `markdown` (wooorm) + `mdast_util_to_markdown` —
  parse → mdast → serialize, frontmatter-aware. Canonical emitter =
  deterministic output; `check` normalizes drift (full round-trip is
  impossible with ASTs, and unneeded — myco owns the format).
- **frontmatter payload**: `toml` (`+++` fences) — repos already
  standardize on `toml`; `indexmap` for stable key order.
- **cli**: `clap` 4 derive + `clap-markdown` (bupha already generates
  CLI reference docs this way → feeds the docs site).
- **git**: shell out to the `git` CLI — the bupha-git/sturn-git
  convention; no git2/gix.
- **index**: `redb` (sturn's pick). **dates**: `time` (sepia's).
  **paths**: `camino`. **errors**: `thiserror` in libs, `miette` in
  cli — `check` diagnostics point at the malformed line/span, ideal
  for agent consumption.
- **templates**: `minijinja`. **slugs**: `slug`. **schema**: `schemars`
  → `myco schema` emits real JSON Schema for free.
- **board render**: `tabled` (derive-based). **tests**: `insta`
  snapshots for md fixtures + `proptest` round-trips (sepia's stack).

## File spec (sketch)

```markdown
# TODO — <repo>
## <section>
- [ ] T-012 short title · pri:high · owner:devin · #label
```

Issues/proposals/ADRs/plans = one md file each, frontmatter (id,
status, created, owner, deps) + templated body. Schema owned by
`myco-store-md`, frozen by testkit fixtures; deterministic output —
same command → same diff, stable ordering, minimal diffs for review.

## Quality bar (bupha anatomy, full)

- `crates/` flat workspace, shared lints, `xtask`, hooks
  (fmt on commit, `xtask check` on push).
- **Docs**: orphan `docs` branch → VitePress + Diátaxis
  (tutorials/how-to/reference/explanation) → GitHub Pages at
  `9aia.com/myco` (Actions deploy on push to `docs`).
- **Coverage**: 100% goal — `xtask coverage` (llvm-cov) +
  `myco-testkit` conformance gating file/state formats.
- Repo anatomy: README/CONTRIBUTING/SECURITY/LICENSE/AGENTS.md,
  `project/` (dogfood via `myco init`), `.agents/skills/`
  (diataxis + rewritten myco skill), `mimus.toml`, `.kuda/`,
  `dist-workspace.toml`, CI.

## Skill rewrite

- `SKILL.md` → thin router: triggers + verb cheat sheet; points
  agents at `myco manifest`/`schema` for discovery.
- `references/` → workflows per command group, not formats.
- `setup.sh` deleted → `myco init`. Rule: agents never hand-edit
  `project/` when a verb exists (if not: edit, then `myco check`).

## Phases

1. **Bootstrap from bupha** — `git branch v0` (skill-only snapshot),
   copy bupha tree → `main` (**files only**, no history graft;
   bupha's history stays in bupha). Rename `bupha-*`→`myco-*`,
   `BUPHA_*`→`MYCO_*`, repo metadata. Delete domain crates (attest/
   queue/runner/trigger/tunnel/dagger/ssh/systemd/github/vcs/access/
   cache/history/status/container/service/cloudflared); keep infra —
   baseline: `core`(shell), `git`, `config(-toml)`, `cli`, `shell`,
   `xtask`; telemetry/secret crates kept or pruned **during the
   prune**, decided per-phase. Dedupe `skills/`+`agent/skills/` →
   `.agents/skills/` (bupha layout). Hand-scaffold `project/`
   worktree + `docs` skeleton (last hand-edit ever). `xtask check` green.
2. `myco-core` + `myco-store-md` — models, ID registry, state
   machine, schemas, golden fixtures (markdown-rs + insta).
3. `myco-git` — `init` / `check` / `migrate` / auto-commit.
4. Item verbs — todo/backlog/issue/proposal/plan/adr + `item set`.
5. `manifest`/`schema`/`status --prompt` + `myco-index-redb` +
   board/report/org views.
6. Continuity — `handoff`/`resume` + sepia session hook.
7. Jira-parity — epics/milestones, `absorb` importers, `serve` ADR.
8. Skill rewrite → pilot on one repo (bupha or sepia) → iterate.
9. Rollout — migrate 7 `pm/` repos, normalize 12 `project/` repos,
   file `~/GitHub/9aia/v2-plan.md` into a `project/plans/`, release
   via cargo-dist.

## Open questions

1. `myco serve` wire — needed for peregrin, or is the CLI enough? [ADR]
2. Index location: `~/.local/share/myco/index.redb` vs per-org config.
3. Jira/GH Projects import formats in `absorb` — scope per source.
4. `myco commit` identity: repo config vs actor-tagged trailers.
5. Signals semantics — item annotations vs defer to sturn entirely.
