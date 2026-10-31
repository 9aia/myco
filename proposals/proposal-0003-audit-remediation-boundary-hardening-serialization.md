+++
id = "P-003"
title = "audit remediation — boundary hardening + serialization"
status = "done"
created = "2026-10-31"
priority = "high"
labels = ["audit"]
+++

## Context

Deep audit (~90 findings across 4 reviewers) found the parse/emit
core disciplined — no unsafe, no injection, errors not panics — but
exposed one root cause: **myco-git is a trust-and-swallow boundary**.
`bool`/`Option` wrappers eat every git error, and verbs trust
`discover` blindly. There is also zero concurrency control in a tool
whose entire pitch is multiple agents writing the same ledger.

## Root cause

- `has_changes`/`is_git_repo`/`git()` map all errors → `false` —
  spawn failure, lock, corrupt repo, missing `git` binary all read
  as "clean" or "not a repo"
- `commit_all` runs `git add -A` repo-wide; `discover` admits
  non-worktree `project/` dirs (`--dir` unchecked, `TODO.md`
  fallback) — verbs then sweep the user's whole repo into `chore:`
  commits
- `create_orphan_branch` uses unconditional `update-ref` — can reset
  `project` to an empty root

## Plan — seven PRs

1. **git boundary** (S): pathspec-scope `add -A`/`status` to the
   worktree; `Result` everywhere instead of `bool`; create-only
   `update-ref <ref> <new> ""`; `qualifies()` on `--dir`;
   commit-identity fallback or targeted preflight error.
2. **serialization** (M): `File::try_lock` on `project/.lock` held
   load→save→commit (std, no deps, stays sync); `repos.toml` folded
   into redb or same-lock RMW; corrupt snapshot → rebuild not fatal;
   canonicalize index keys at `add`. Optional belt: CAS HEAD via
   `update-ref <branch> <new> <old>` for lock-free retry.
3. **integrity** (M): `deny_unknown_fields` + counters ≥1 on
   meta.toml (typo → id reuse today); `all_ids` → `Result`;
   filename↔frontmatter id cross-check; `emit_item_file` validates
   like `line::check_item`; `TaskLoc` panics → `Option`;
   validate-then-apply for `walk_to`/`NewItemMeta::apply`.
4. **check --fix safety** (M): no silent rename-replace; missing ≠
   unreadable (never scaffold over unreadable files); renumber
   repoints `handoffs/archive/` + supersede refs; parse-failed files
   excluded from the semantic pass.
5. **discovery + escaping** (S): cap ancestor walk at repo toplevel;
   `project_dir` rejects `..`/absolute, read myco.toml from repo
   root; `mdx::inline` validates in real emit context (`## x ###`
   breaks round-trip); tighten `is_valid_session`; testkit charsets
   gain `#`/whitespace.
6. **TUI** (M): mutations → status bar not exit; ITEMS board
   read-only enforced centrally; per-mutation re-read under the
   PR-2 lock (kills stale-ledger clobber); terminal `Guard`
   ordering; global `Ctrl-C`; `$EDITOR` split.
7. **JSON parity** (XS): `status --prompt --json`, `board --json`
   `interrupted`, `org` errors carry path, `--blocks` delimiter on
   `item set`.

## Non-goals / boundary

sturn and peregrin stay out — every bug is local FS, and pulling a
daemon into a local race violates the boundary. Their real fit:
sturn session hooks → `myco interrupt`; peregrin owns `myco serve`
(ADR-0001) where multi-clone serialization would live.

## Order

PR-1+2 (data safety) → PR-3 (integrity) → PR-4–7. Full finding list
in session audit; issues to be filed per PR on acceptance.
