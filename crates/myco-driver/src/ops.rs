//! Engine ops — `cli.<verb>` wire queries dispatch into the `myco`
//! engine lib through this binary's own `run` verb: each call
//! re-execs `tamias-driver-myco run <argv>` with the request's repo
//! as cwd, so `myco::dispatch` writes to the child's captured
//! stdout and never touches the serve loop's ndjson stream — and
//! every mutation keeps the engine's own write-path semantics
//! (per-verb commit, `Actor:` trailer, worktree lock).
//!
//! `ops` answers the ops manifest as JSON — derived from the
//! engine's own [`myco::registry::VERBS`] table, so a verb can't
//! exist on the CLI and be missing here. Unbounded verbs
//! (`serve`, `ui`) are deliberately *not* ops — they never
//! return, so they live in `surfaces` instead: surface drivers
//! (`tamias-driver-cli` / `-mcp`, the pavo-driver-kit `surface`
//! layer) expose them as streaming subcommands.
//!
//! Wire params carry each verb's declared param schema: `arg` =
//! positional, `opt` = `--name <v>`, `flag` = `--name` — the
//! contract `pavo-driver-kit`'s surface layer renders args from.

use std::path::PathBuf;
use std::process::Output;

use myco::registry::{self, On, PKind, Val, Verb};
use myco_core::{Priority, Status};
use pavo_driver_kit as kit;
use pavo_protocol::wire::{ActionResult, QueryResult, Response, SubmitResult};
use pavo_protocol::{ParamKind, ParamSchema};
use serde_json::{json, Map, Value};

/// Unbounded surfaces — verbs that never return, so they can
/// never be request/response ops. The CLI/MCP surfaces stream
/// them via `run` passthrough instead.
const SURFACES: &[&str] = &["serve", "ui"];

fn is_surface(name: &str) -> bool {
    SURFACES.contains(&name)
}

/// The ops table — every CLI verb minus hidden and surfaces.
fn ops() -> impl Iterator<Item = &'static Verb> {
    registry::VERBS
        .iter()
        .filter(|v| v.cli.is_some() && !v.hidden && !is_surface(v.name))
}

/// Status tokens for enum params — the full state machine, in
/// declaration order.
pub(crate) fn status_options() -> Vec<String> {
    Status::ALL.iter().map(|s| s.token().to_string()).collect()
}

/// Priority tokens — `low|med|high|crit` (the `pri:` spellings the
/// engine accepts canonically).
pub(crate) fn pri_options() -> Vec<String> {
    Priority::ALL
        .iter()
        .map(|p| p.token().to_string())
        .collect()
}

/// `Param` → the typed [`ParamSchema`] carried on the ops
/// manifest's `schema` field — hosts auto-generate action forms
/// from it (enum pickers, ref pickers, validation). Driven by the
/// param's `Val` type; a few name-driven upgrades for the common
/// text fields (`title`/`note` → text, `due` → date).
fn param_schema(p: &registry::Param) -> ParamSchema {
    let mut b = kit::param(p.name).doc(p.help);
    if p.required {
        b = b.required();
    }
    let b = if p.kind == PKind::Flag {
        b.kind(ParamKind::Bool)
    } else if p.multi && matches!(p.val, Val::Str | Val::Path | Val::Actor | Val::Id) {
        // Repeatable value lists (`--label`, `--blocks`, …) are
        // token inputs, not single pickers.
        b.kind(ParamKind::StringList)
    } else {
        match p.val {
            Val::Id => b.kind(ParamKind::Ref).ref_query("items"),
            Val::Status => b.enumeration(status_options()),
            Val::Pri => b.enumeration(pri_options()),
            // The five prelude kind names — `[[kinds]]` config can
            // add more, but the enum documents the common case.
            Val::Kind => b.enumeration(
                ["task", "issue", "proposal", "adr", "plan"]
                    .iter()
                    .map(|s| (*s).to_string())
                    .collect(),
            ),
            Val::Count => b.kind(ParamKind::Integer),
            Val::Source => b.enumeration(vec!["repo".into(), "index".into()]),
            Val::Axis => b.enumeration(
                [
                    "status",
                    "priority",
                    "kind",
                    "owner",
                    "epic",
                    "milestone",
                    "repo",
                ]
                .iter()
                .map(|s| (*s).to_string())
                .collect(),
            ),
            Val::Format => b.enumeration(vec!["list".into(), "board".into(), "repos".into()]),
            Val::Str | Val::Path | Val::Actor | Val::Sort | Val::Spec => {
                if matches!(p.name, "title" | "note" | "text" | "body" | "about") {
                    b.text()
                } else if p.name == "due" {
                    b.kind(ParamKind::Date)
                } else if p.name == "delta" {
                    // `item reorder --delta` — signed sibling steps.
                    b.kind(ParamKind::Integer)
                } else {
                    b
                }
            }
        }
    };
    b.build()
}

/// `Param` → the manifest's typed param — `arg`/`opt`/`flag` as
/// `pavo-driver-kit`'s surface layer spells them, plus the
/// [`ParamSchema`] (`schema`) hosts use for action auto-forms.
/// Wire-only params (`Spec` patches) can't ride argv, so they stay
/// out — surfaces only offer what `run` can pass.
fn param_json(p: &registry::Param) -> Value {
    json!({
        "name": p.name,
        "kind": match p.kind {
            PKind::Pos => "arg",
            PKind::Opt => "opt",
            PKind::Flag => "flag",
        },
        "doc": p.help,
        "schema": serde_json::to_value(param_schema(p)).unwrap_or(Value::Null),
    })
}

/// The `ops` query/`ops` argv verb payload — self-describing so
/// hosts and agents discover the engine surface without argv help.
/// `json`/`no_commit`/`dir`/`root` are the engine's globals —
/// accepted by every op, listed once on the manifest itself.
pub fn manifest() -> Value {
    json!({
        "engine": "myco",
        "invoke": "query `cli.<verb>` with the listed params, or argv `tamias-driver-myco run <myco argv>`",
        "globals": [
            {"name": "json", "kind": "flag", "doc": "Emit JSON (accepted everywhere)."},
            {"name": "no_commit", "kind": "flag", "doc": "Skip the per-verb commit (also suppressed by $MYCO_NO_COMMIT)."},
            {"name": "dir", "kind": "opt", "doc": "Project worktree to act on, overriding discovery."},
            {"name": "root", "kind": "opt", "doc": "Repo root to act on (the worktree is `<root>/project`)."},
        ],
        "ops": ops().map(|v| json!({
            "query": format!("cli.{}", v.name),
            "doc": v.about,
            "params": v.params()
                .filter(|p| p.on != On::Wire)
                .map(param_json)
                .collect::<Vec<_>>(),
            "mutates": v.mutates,
        })).collect::<Vec<_>>(),
        "surfaces": registry::VERBS.iter()
            .filter(|v| v.cli.is_some() && is_surface(v.name))
            .map(|v| v.name)
            .collect::<Vec<_>>(),
    })
}

/// Is `query` an engine op (`cli.<verb>`)? Returns the verb.
pub fn op_of(query: &str) -> Option<&'static Verb> {
    query
        .strip_prefix("cli.")
        .and_then(registry::find)
        .filter(|v| v.cli.is_some() && !v.hidden && !is_surface(v.name))
}

fn bool_param(params: &Map<String, Value>, key: &str) -> bool {
    params.get(key).and_then(Value::as_bool).unwrap_or(false)
}

/// A wire scalar → its argv token (numbers/bools stringify —
/// same read `Args::from_json` gives them).
fn scalar(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// All values of a param — arrays flatten, scalars wrap.
fn values(v: &Value) -> Vec<String> {
    match v {
        Value::Array(xs) => xs.iter().filter_map(scalar).collect(),
        other => scalar(other).into_iter().collect(),
    }
}

/// `cli.<verb>` params → myco argv (no argv[0] — `run` prepends
/// it). Globals go first; the dotted verb name splits into the
/// group path (`todo.add` → `todo add`). Declared params map by
/// kind — positionals land last, in schema order.
fn argv(v: &Verb, params: &Map<String, Value>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    if bool_param(params, "json") {
        out.push("--json".into());
    }
    if bool_param(params, "no_commit") || bool_param(params, "no-commit") {
        out.push("--no-commit".into());
    }
    // `dir` wins over its `project` alias; `root` is separate —
    // same precedence `data::load` resolves the project by.
    let dir = params
        .get("dir")
        .or_else(|| params.get("project"))
        .and_then(Value::as_str);
    if let Some(d) = dir {
        out.extend(["--dir".into(), d.into()]);
    }
    if let Some(r) = params.get("root").and_then(Value::as_str) {
        out.extend(["--root".into(), r.into()]);
    }
    out.extend(v.name.split('.').map(String::from));
    let mut positional: Vec<String> = Vec::new();
    for p in v.params() {
        if p.on == On::Wire {
            continue;
        }
        // canonical underscore name, dashed alias tolerated
        let raw = params
            .get(p.name)
            .or_else(|| params.get(p.name.replace('_', "-").as_str()));
        let Some(raw) = raw else { continue };
        match p.kind {
            PKind::Pos => positional.extend(values(raw)),
            PKind::Flag => {
                if raw.as_bool().unwrap_or(false) {
                    out.push(format!("--{}", p.name.replace('_', "-")));
                }
            }
            PKind::Opt => {
                for s in values(raw) {
                    out.push(format!("--{}", p.name.replace('_', "-")));
                    out.push(s);
                }
            }
        }
    }
    out.extend(positional);
    out
}

/// The child's working dir — the steering params first
/// (`dir`/`project`/`root`), then `$MYCO_DIR`, then the driver's
/// own cwd (pavo inherits the user's cwd, so a bare `cli.status`
/// inside repo X reads X's ledger).
fn workdir(params: &Map<String, Value>) -> PathBuf {
    for key in ["dir", "project", "root"] {
        if let Some(s) = params.get(key).and_then(Value::as_str) {
            return PathBuf::from(s);
        }
    }
    if let Ok(d) = std::env::var("MYCO_DIR") {
        if !d.is_empty() {
            return PathBuf::from(d);
        }
    }
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

/// Run one engine op by name: re-exec `run` in a child whose cwd
/// is the request's repo, and hand back `{op, argv, code, stdout,
/// stderr}` as the query payload. `ok:false` is reserved for
/// spawn failures — a nonzero `code` is the engine's own exit
/// status (e.g. a `check` finding), honest data. The bare-name
/// entry the embedded surfaces use (`OpSpec.name` is
/// `cli.`-stripped); the wire path goes `op_of` → `run_verb`.
#[cfg(feature = "mcp")]
pub fn run_op(op: &str, params: &Map<String, Value>) -> Response {
    let Some(v) = op_of(&format!("cli.{}", op.strip_prefix("cli.").unwrap_or(op))) else {
        return Response::err(format!("unknown op: {op}"));
    };
    run_verb(v, params)
}

/// argv + child exec — shared by `cli.<verb>` queries ([`run_verb`])
/// and `invoke` ([`invoke`]). The child is this binary's own `run`
/// verb in the request's repo, so engine output never touches the
/// serve stream.
fn exec(v: &Verb, params: &Map<String, Value>) -> Result<(Vec<String>, Output), String> {
    let args = argv(v, params);
    let out = std::env::current_exe().and_then(|exe| {
        std::process::Command::new(exe)
            .arg("run")
            .args(&args)
            .current_dir(workdir(params))
            .output()
    });
    match out {
        Ok(o) => Ok((args, o)),
        Err(e) => Err(format!("run {}: {e}", v.name)),
    }
}

/// [`run_op`] with the verb already resolved — the `cli.<verb>`
/// wire-query path lands here too.
pub fn run_verb(v: &Verb, params: &Map<String, Value>) -> Response {
    let (args, o) = match exec(v, params) {
        Ok(x) => x,
        Err(e) => return Response::err(e),
    };
    let data = json!({
        "op": v.name,
        "argv": args,
        "code": o.status.code().unwrap_or(-1),
        "stdout": String::from_utf8_lossy(&o.stdout),
        "stderr": String::from_utf8_lossy(&o.stderr),
    });
    Response::ok_typed(&QueryResult {
        query: format!("cli.{}", v.name),
        data,
    })
}

// ---------------------------------------------------------------- invoke
//
// The pavo interaction protocol's `invoke`/`submit` ops. Action ids
// ARE verb names — the declared `actions[]` (views::manifest) and
// the ops-derived fallback both spell ids as `todo.done`,
// `item.set`, …; a `cli.` prefix is tolerated for symmetry with the
// query spelling. Dispatch rides the exact `cli.<verb>` path:
// params → argv → `run` child → engine semantics (per-verb commit,
// actor trailers, worktree lock) are unchanged.

/// Resolve an action id to an engine verb — bare names and the
/// `cli.`-prefixed ops spelling both work; view ids, hidden verbs,
/// and surfaces are not actions.
pub fn action_verb(action: &str) -> Option<&'static Verb> {
    let name = action.strip_prefix("cli.").unwrap_or(action);
    registry::find(name).filter(|v| v.cli.is_some() && !v.hidden && !is_surface(v.name))
}

/// The `invoke` target→params merge rule: an object target
/// contributes keys the params lack (explicit params win — the
/// form's answers override row context); a scalar target fills
/// `id` when the verb declares one and params don't (tree-item
/// actions send the row id this way). Anything else is ignored.
fn merge_target(
    v: &Verb,
    params: &Map<String, Value>,
    target: Option<&Value>,
) -> Map<String, Value> {
    let mut out = params.clone();
    match target {
        Some(Value::Object(t)) => {
            for (k, val) in t {
                out.entry(k.clone()).or_insert_with(|| val.clone());
            }
        }
        Some(t) => {
            let takes_id = v.params().any(|p| p.name == "id" && p.on != On::Wire);
            if takes_id && !out.contains_key("id") {
                if let Some(id) = scalar(t) {
                    out.insert("id".to_string(), json!(id));
                }
            }
        }
        None => {}
    }
    out
}

/// A param value counts as present when it's a non-empty scalar or
/// a non-empty array — `{"title":""}` is as missing as absent.
fn present(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(_) => true,
    }
}

/// Params the verb can't run without — `required` flags on
/// argv-ridable params (positionals default required). Wire-only
/// params never reach argv, so they don't count; dashed aliases
/// satisfy the canonical name.
fn missing_required(v: &Verb, params: &Map<String, Value>) -> Map<String, Value> {
    let mut errors = Map::new();
    for p in v.params() {
        if p.on == On::Wire || !p.required {
            continue;
        }
        let ok =
            present(params.get(p.name)) || present(params.get(p.name.replace('_', "-").as_str()));
        if !ok {
            errors.insert(p.name.to_string(), json!("required"));
        }
    }
    errors
}

/// First non-empty line — the human-facing message for an
/// [`ActionResult`]/[`SubmitResult`].
fn first_line(s: &str) -> Option<String> {
    s.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(str::to_string)
}

/// `{"op":"invoke"}` — dispatch `action` (a verb id) through the
/// same argv/exec path `cli.<verb>` queries use. Unknown actions
/// and spawn failures are `Err` (wire `ok:false`); verb outcomes —
/// including nonzero exits — come back `Ok` with `{code, stdout,
/// stderr}` in `data`, mirroring how `cli.*` queries report the
/// engine's own exit status as honest data. `view` is context only
/// (node/item-scoped action resolution is a host concern); repo
/// targeting stays `dir`/`root`/cwd like every other op.
pub fn invoke(
    action: &str,
    target: Option<&Value>,
    params: &Map<String, Value>,
) -> Result<ActionResult, String> {
    let Some(v) = action_verb(action) else {
        return Err(format!("unknown action: {action}"));
    };
    let merged = merge_target(v, params, target);
    let errors = missing_required(v, &merged);
    if !errors.is_empty() {
        return Ok(ActionResult {
            errors,
            ..ActionResult::default()
        });
    }
    let (args, o) = exec(v, &merged)?;
    let code = o.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&o.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&o.stderr).into_owned();
    let message = if code == 0 {
        first_line(&stdout).unwrap_or_else(|| format!("{} ok", v.name))
    } else {
        first_line(&stderr).unwrap_or_else(|| format!("{} failed (exit {code})", v.name))
    };
    Ok(ActionResult {
        message: Some(message),
        data: Some(json!({
            "op": v.name,
            "argv": args,
            "code": code,
            "stdout": stdout,
            "stderr": stderr,
        })),
        ..ActionResult::default()
    })
}

/// `{"op":"submit"}` — thin adapter. This driver declares no form
/// nodes, so a submit only dispatches when `form` (or the view id
/// itself, when no form is named) resolves to an engine verb —
/// the submitted `values` become the verb params verbatim. Anything
/// else is `ok:false`: there is no form to validate against.
pub fn submit(view: &str, form: Option<&str>, values: &Map<String, Value>) -> Response {
    let candidate = form.unwrap_or(view);
    if action_verb(candidate).is_none() {
        return Response::err(format!(
            "no form '{candidate}' on view '{view}' — this driver declares no forms"
        ));
    }
    match invoke(candidate, None, values) {
        Ok(r) => Response::ok_typed(&SubmitResult {
            message: r.message,
            errors: r.errors,
            data: r.data,
            view: r.view,
            extra: r.extra,
        }),
        Err(e) => Response::err(e),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use serde_json::json;

    #[test]
    fn op_of_accepts_cli_prefix_only() {
        assert_eq!(op_of("cli.status").unwrap().name, "status");
        assert_eq!(op_of("cli.todo.add").unwrap().name, "todo.add");
        assert!(op_of("cli.serve").is_none()); // surface, not an op
        assert!(op_of("cli.ui").is_none());
        assert!(op_of("cli.gen-cli-docs").is_none()); // hidden
        assert!(op_of("status").is_none()); // view query, not an op
        assert!(op_of("cli.bogus").is_none());
    }

    #[test]
    fn argv_maps_params_to_myco_flags() {
        let v = registry::find("todo.add").unwrap();
        let mut p = Map::new();
        p.insert("title".into(), json!("fix it"));
        p.insert("pri".into(), json!("high"));
        p.insert("label".into(), json!(["a", "b"]));
        p.insert("json".into(), json!(true));
        p.insert("root".into(), json!("/repo"));
        assert_eq!(
            argv(v, &p),
            vec![
                "--json", "--root", "/repo", "todo", "add", "--pri", "high", "--label", "a",
                "--label", "b", "fix it",
            ]
        );
    }

    #[test]
    fn argv_skips_wire_only_and_absent_params() {
        let v = registry::find("view").unwrap();
        let mut p = Map::new();
        p.insert("name".into(), json!("board"));
        p.insert("spec".into(), json!({"limit": 5})); // wire-only — can't ride argv
        assert_eq!(argv(v, &p), vec!["view", "board"]);
    }

    #[test]
    fn manifest_lists_every_op_as_cli_query() {
        let m = manifest();
        let ops = m["ops"].as_array().unwrap();
        for v in registry::VERBS
            .iter()
            .filter(|v| v.cli.is_some() && !v.hidden && !is_surface(v.name))
        {
            assert!(
                ops.iter()
                    .any(|o| o["query"] == json!(format!("cli.{}", v.name))),
                "{} missing from ops manifest",
                v.name
            );
        }
        assert!(m["surfaces"].as_array().unwrap().contains(&json!("serve")));
        for o in ops {
            for p in o["params"].as_array().unwrap() {
                assert!(["arg", "opt", "flag"].contains(&p["kind"].as_str().unwrap()));
            }
        }
    }
}
