# Declare custom item kinds

`[[kinds]]` tables in `myco.toml` extend the five prelude kinds
(task, issue, proposal, adr, plan) with your own — a `spike`
folder of experiment write-ups, a `decision` log, whatever the
ledger needs. `myco init` seeds the whole prelude as comments in
the repo-root `myco.toml`; uncomment a block to restate a builtin,
or append entries to declare new kinds.

## Declare one

```toml
# myco.toml (repo root — or ~/.config/myco/myco.toml for all repos)

[[kinds]]
name      = "spike"
prefix    = "S"               # [A-Z][A-Z0-9]*, ≤ 15 bytes — S-001 is now a real id
storage   = "dir"             # dir = one file per item; doc = ledger lines
dir       = "spikes"          # optional — defaults to <name>s
done_state = "done"           # the status that resolves the item
zero_pad  = 3                 # S-001 vs ADR-0001
```

Field reference (all but `name`/`prefix`/`storage` optional —
full list in [config](../reference/config.md)):

| field          | meaning                                                        |
| -------------- | -------------------------------------------------------------- |
| `name`         | vocabulary name — `kind = ["spike"]` in views, `S-` ids        |
| `prefix`       | id prefix — must not collide with another kind's prefix        |
| `storage`      | `doc` (ledger lines) or `dir` (one md file per item)           |
| `dir`          | `dir` storage subdirectory — defaults to `<name>s`             |
| `file_prefix`  | filename prefix — defaults to `name`; `""` gives `0004-slug.md` |
| `states`       | the statuses this kind may carry — defaults to the full machine |
| `done_state`   | the resolving status — `done` unless declared otherwise        |
| `supersede`    | `superseded_by:`/`supersedes:` frontmatter is meaningful       |
| `resolve_verb` | declaration only in v1 — no verb reads it yet                  |
| `zero_pad`     | digits in emitted ids — 3 default                              |

## What you get for free

A declared kind rides the generic surface — no new verbs:

- **files** — `dir` kinds get `spikes/spike-NNNN-<slug>.md` with
  the same `+++` frontmatter + markdown body as issues and
  plans ([file spec](../explanation/file-spec.md)); `doc` kinds
  live in the ledger docs
- **verbs** — `myco item show S-001`, `item set` (status walks
  *this kind's* `states`), `item move`, `handoff`, `resume`,
  `interrupt` all work on any declared id
- **views** — `kind = ["spike"]` (or `["S"]`) filters any view;
  `group_by = "kind"` buckets mixed ledgers
- **checks** — `myco check` validates the kind's files, ids, and
  state machine, and lints the `[[kinds]]` table itself
  (`kind-invalid` findings)

## Restating a prelude kind

Naming a `[[kinds]]` entry `issue` *shadows* the builtin spec —
restating it verbatim is a no-op (that's what the commented seed
shows), and changing fields like `states` or `done_state`
reconfigures the kind. The `prefix` is locked: `issue`'s `I` is
identity — reinterpreting it would rewrite every existing id, so
a mismatched prefix is a config error. Repo `myco.toml` wins a
name collision over `~/.config/myco/myco.toml` — a repo's kind
vocabulary is shared surface.

## Creating items

There are no per-kind `new` verbs for custom kinds (the prelude
kinds keep theirs — `issue new`, `plan new`, …). Author the item
file per the [file spec](../explanation/file-spec.md) —
`spikes/spike-0001-<slug>.md` with `id = "S-001"` frontmatter —
then `myco check` validates it and the verbs take over from
there. Keep the `[next]` counters in `project/meta.toml` ahead of
the ids you hand out, or the next `alloc` collides.
