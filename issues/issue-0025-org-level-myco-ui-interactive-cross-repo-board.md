+++
id = "I-025"
title = "Org-level myco ui — interactive cross-repo board"
status = "in-progress"
created = "2026-10-31"
owner = "devin"
priority = "medium"
labels = ["org", "tui"]
+++

`myco ui` is single-repo only: it resolves one `project/` worktree via `discover()` (`--dir`/`--root`), and the TUI model holds one `MdStore` + per-worktree lock. Running it above a repo (e.g. a `9aia/`-style org dir, which isn't even a git repo) fails at discovery.

Cross-repo today is read-only and non-interactive: `myco org status` / `myco org board` merge index snapshots (auto-rebuilding stale ones) — no editing, and snapshots carry no handoffs.

## Options

- **Read-only org TUI** — ratatui over `org::load_fresh()`. Simplest; can't mutate (index is a disposable cache, not the store).
- **Repo-picker front-end** — org overview → drill into a repo → existing single-ledger board. Keeps the locking/mutation model intact; the natural path to a *writable* org UI.

Open questions: should mutations route through the index (needs handoffs/snapshots enriched) or stay per-worktree? Does this belong in `myco-tui` or is a multi-repo view a peregrin concern per the boundary in AGENTS.md?
