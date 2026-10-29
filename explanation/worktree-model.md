---
url: /myco/explanation/worktree-model.md
---
# The worktree model

Why `project/` is a git worktree of an orphan branch — and what
that buys you.

## The shape

```text
your-repo/
├── src/ …            ← code branch (main)
├── project/          ← worktree of the orphan `project` branch
│   ├── TODO.md
│   ├── BACKLOG.md
│   ├── issues/  proposals/  plans/  adrs/  handoffs/
│   ├── index.md
│   └── meta.toml
└── .gitignore        ← gains `project/`
```

The `project` branch shares the repo's `.git` object store but
has disjoint history — an orphan root, no code ancestors. The
worktree mounts it as a plain directory, so tools see ordinary
files while `git worktree list` shows the truth.

## Why an orphan branch

PM state moves at its own cadence. On an orphan branch, ledger
commits never touch the code history: no `chore: defer T-009`
noise in `main`'s log, no PM diffs in PRs, no branch protection
friction. `project/` is gitignored on every code branch — the
worktree physically can't leak into a code commit.

## Why a worktree (not just a branch)

The ledger has to be *visible* — agents edit files, humans read
`TODO.md` — without `git checkout` swapping the code tree. A
worktree keeps both checked out at once.

## Commits are the audit trail

Every mutating verb is one atomic, conventional commit on
`project` (`chore: add T-012`, `chore: handoff T-012`,
`chore: resolve I-003`, …). So
`git -C project log` *is* the history of intent — who moved what
when, replayable and greppable, with no separate audit store.
`--no-commit` / `$MYCO_NO_COMMIT` opts out per run; `myco commit`
squashes a batch into one.

## Canonical markdown, disposable index

The markdown in `project/` is canonical — human-editable,
diffable, frozen by golden fixtures. The redb index
(`index.redb` + `repos.toml` under `$MYCO_HOME` →
`$XDG_DATA_HOME/myco` → `~/.local/share/myco`) is a *derived
cache* that powers `status`, `board`, `report`, and `org` without
re-parsing files. Delete it and `myco index rebuild` derives it
back; `myco check` finds and repairs drift between the two.

See [the file spec](file-spec.md) for what the files contain and
[architecture](architecture.md) for the crates that implement it.
