---
url: /myco/reference/config.md
---
# Config — `myco.toml`

## Top level

| field | type | required | notes |
|---|---|---|---|
| `project_dir` | `string` | optional | Path to the project worktree, relative to the repo root. |

## Constraints

* `deny_unknown_fields`: typos in field names fail the load.
* Search order: `$MYCO_CONFIG` → `./myco.toml` →
  `~/.config/myco/myco.toml`. The file is optional — no
  `myco.toml` anywhere just means defaults.
