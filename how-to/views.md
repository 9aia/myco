---
url: /myco/how-to/views.md
---
# Shape the board with named views

A view is a declarative spec — `filter`, `group_by`, `sort`,
`limit`, `format` — evaluated by one engine. Names come from two
places: built-in presets (`board`, `open`, `epic`, `milestone`)
and `[views.<name>]` tables in `myco.toml`
([spec fields](../reference/config.md)).

## Define one

```toml
# myco.toml
[views.hot]
filter = { pri_min = "high", status = ["open", "in-progress"] }
group_by = "owner"
sort = "-created"
format = "list"
```

```sh
myco view hot          # resolves the named view
myco view              # lists every defined view + origin
myco view --status open --pri-min high   # ad-hoc spec from flags
myco view hot --limit 3                  # flags patch the named spec
```

Every read is also `--json`: `{view, spec, groups, interrupted}`.

## The verbs are verb-fronted views

`myco board`, `myco epic`, `myco milestone` and `myco status`
resolve the same-named view before rendering — so a config view
shadowing a built-in name redefines the verb itself:

```toml
[views.board]
filter = { kind = ["issue"] } # myco board is now issues-only
```

A shadow warns on stderr (`view "board" shadows the built-in
preset`) and `myco check` lints the repo layer's specs. Under a
shadowed spec the legacy chrome falls back to the generic
renderers: a non-status `group_by` renders its own columns, and
`--json` emits the `myco view` payload instead of the five-lane
map. Specs that admit non-lane buckets (e.g. `done`) surface them
as extra columns/keys — nothing matched is silently dropped.

Layering is repo `<` user `<` flags: a `[views]` table merges
across config layers; scalar fields stay first-match.

## Over the wire

`myco serve` exposes `view` (`{name?, spec?}` — `spec` is the
same per-field patch) alongside the rest of the verb surface;
`myco batch` reaches all of it one-shot:

```sh
printf '{"id":1,"method":"view","params":{"name":"hot"}}\n' | myco batch
```
