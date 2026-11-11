---
url: /myco/reference/config.md
---
# Config — `myco.toml`

## Top level

| field | type | required | notes |
|---|---|---|---|
| `project_dir` | `string` | optional | Path to the project worktree, relative to the repo root. |
| `default_view` | `string` | optional | The named view bare `myco view` resolves when no name is given — a repo pins its landing view, a machine `~/myco.toml` pins a global one (scalar: repo layer wins). |
| `views` | `map` | optional | Named declarative views — merged across config layers (P-006): repo `myco.toml` presets are shared, user-config presets personal; the *user* layer wins a name collision. |
| `verbs` | `map` | optional | Config-declared verbs (P-007) — merged like `views`, but the *repo* layer wins collisions (repo verbs are shared surface, personal verbs don't override them). Builtins win over both at resolution. |
| `kinds` | `Vec<KindSpec>` | optional | `[[kinds]]` — user item kinds layered over the prelude (P-008). A prelude name *shadows* that spec (prefix stays locked); new names extend the table. Merged like verbs: the repo layer wins a name collision — a repo's kind vocabulary is shared surface. |
| `sync` | `SyncConfig` | optional | `[sync]` — ledger-branch sync policy. `None` = defaults. Repo layer wins the table like the scalars. |
| `index` | `IndexConfig` | optional | `[index]` — org-index designation (`ledger = "org"\|"none"`). The index universe reads the repo-layer `myco.toml` directly, so a user-layer table can't leak a global designation into every repo. |

## Constraints

* `deny_unknown_fields`: typos in field names fail the load.
* Search order: `$MYCO_CONFIG` → `./myco.toml` →
  `~/.config/myco/myco.toml`. The file is optional — no
  `myco.toml` anywhere just means defaults.
