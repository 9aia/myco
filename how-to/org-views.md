# See work across repos

`myco index` registers repos into a machine-wide cache;
`myco org` renders cross-repo views over it.

## The index

The index lives under `$MYCO_HOME`, else `$XDG_DATA_HOME/myco`,
else `~/.local/share/myco`:

- `repos.toml` — the registry (`[[repo]] path name`), a plain
  file you can inspect
- `index.redb` — one `snapshots` table: repo path → JSON snapshot

It is a **derived, disposable cache** — markdown in each repo's
`project/` stays canonical. `rm -rf` the index dir and
`myco index rebuild` derives it back.

## Register repos

```sh
myco index add ~/GitHub/9aia/sepia    # repo root or its project/ worktree
myco index add .                      # register the repo you're in
myco index list                       # repos + snapshot freshness
```

`add` takes an initial snapshot. Snapshots go stale as ledgers
change — `list` marks them `stale`; `org` verbs auto-rebuild
stale entries, or rebuild explicitly:

```sh
myco index rebuild                    # all registered repos
myco index rebuild ~/GitHub/9aia/sepia
myco index remove ~/GitHub/9aia/sepia # unregister + drop snapshot
```

## Org views

```sh
myco org status
```

```text
org status — 21 repo(s)
bupha            tasks 7o/0c/0ip/0b backlog 15 issues 0o p 0o pl 0o adr 2o  @ad8b625
sepia            tasks 2o/0c/0ip/0b backlog 4 issues 0o p 0o pl 1o adr 0o  @8cbc633
...
totals: tasks 167o/0c/1ip/0b backlog 326 issues 1o p 5o pl 7o/1r adr 59o
```

Per-repo counts (`o`pen / `c`laimed / `i`n-`p`rogress /
`b`locked), the snapshot commit, org totals, then items grouped
by owner. A rebuilt repo is marked `(rebuilt)`.

```sh
myco org board          # org kanban — repo-prefixed cards over every live item
myco org board --md     # markdown table for docs/READMEs
```

Cards are prefixed with the repo name (`thal:T-118 …`) so one
board spans the whole org.
