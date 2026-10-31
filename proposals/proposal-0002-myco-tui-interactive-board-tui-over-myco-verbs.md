+++
id = "P-002"
title = "myco-tui — interactive board TUI over myco verbs"
status = "open"
created = "2026-10-31"
owner = "devin"
priority = "high"
labels = ["tui"]
+++

## Context

`9aia/pm` proved the interaction model (Bubble Tea TUI over `project/`).
pm is now stripped to a runnable TUI reference (`docs/tui.md` there is
the porting doc); everything else it did is superseded by myco. This
proposal ports the **UX design** — not code — to a `myco-tui` crate.

## Shape

- `crates/myco-tui` — ratatui + crossterm (sync; no async runtime).
  An adapter crate like myco-store-md: thin front-end, zero ledger
  logic.
- `myco ui` subcommand in myco-cli, behind cargo feature `tui`
  (default on). `--no-default-features` keeps the lean agent binary.
- Every write goes through the same ops the verbs use — commit-per-verb
  via myco-git stays the audit path. The TUI never touches files.

## Phases

0. **Shared ops**: lift verb logic out of `myco-cli/src/{todo,backlog,
   itemcmd}.rs` into a reusable ops layer (myco-core or a new
   `myco-ops` crate) so both `main.rs` and `myco-tui` call it.
1. **Read-only boards**: TODO/BACKLOG tabs, render whole-file markdown,
   cursor↔item selection via parsed spans, item preview (body/note).
2. **`myco ui` wired** (feature-gated) + write verbs one at a time:
   `Space`→done, `dd`→archive-equivalent (status transition),
   `Alt+↑/↓` reorder, `o`/`O` add.
3. **Polish + retire pm**: search/filter, `E`/`gf` editor, `R` reload,
   session restore; delete `9aia/pm` once dogfooding is clean.

## Keymap (from pm reference)

`h/l` tabs · `j/k` scroll · `gg`/`G` · `1/2/3` jump · `F` filter ·
`/`+`n`/`N`, `*` search · `Space` toggle · `Alt+↑/↓` reorder · `M`
section move · `dd`/`u` · `o`/`O` insert · `Enter`/`esc` preview ·
`E`/`gf` $EDITOR · `yy` copy · `R` reload · `q` quit.

## Mapping

pm `app.*` writes → myco ops; `notes/<slug>.md` → item files/body;
`archived/` → `done` status; pm sync → myco-git auto-commit;
Glamour → pulldown-cmark→`Line` renderer (or termimad).
Boards: TODO/BACKLOG today, item-file kinds (issues/proposals/plans/
adrs) as later tabs.
