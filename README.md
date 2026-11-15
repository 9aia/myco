# myco

*Records engine for agent work — views + mutations; project management
is the shipped prelude.*

Agent-first PM in Rust. A single `myco` CLI owns every deterministic
write to a repo's `project/` worktree; the skill teaches agents the
verbs, not file formats. Markdown is canonical and human-editable;
history on the `project` branch is the audit trail.

```
agent ──► myco <verb> ──► write md ──► commit on `project` branch
              │                        (one atomic commit per verb)
              └──► --json everywhere + manifest/schema self-discovery
```

## Quickstart

```sh
myco init                     # scaffold project/ worktree in a repo
myco todo add "ship the thing"
myco status --prompt          # token-sized board digest
myco resume T-012             # pick work back up
myco handoff T-012            # attach resumable state at session end
```

The `myco` command ships as a tamias `bins[]` shim —
`tamias install 9aia:myco@<version>`, or `cargo xtask pack && tamias
install --allow-drivers ./libraries/myco` from a checkout. See
[Install](https://9aia.com/myco/getting-started) for details.

## Position in the stack

myco = **work memory** — the ledger of intent (todos, issues,
proposals, plans, ADRs, handoffs). Neighbors own theirs: **sturn**
(ephemeral coordination), **kuda** (code memory), **peregrin**
(orchestration), **bupha**/**ci-gate** (CI gate), **mycena**
(TODO/FIXME surfacing), **sepia** (session lifecycle →
handoff/resume).

## Docs

- [Docs site](https://9aia.com/myco) — tutorials, how-tos, reference,
  architecture (VitePress + Diátaxis, `docs/` worktree)
- [AGENTS.md](AGENTS.md) — crate map, commands, rules, boundary
- [CONTRIBUTING.md](CONTRIBUTING.md) · [SECURITY.md](SECURITY.md)
- [LICENSE](LICENSE) (MIT) · [project ledger](project/index.md)
