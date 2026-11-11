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

use myco::registry::{self, On, PKind, Verb};
use pavo_protocol::wire::{QueryResult, Response};
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

/// `Param` → the manifest's typed param — `arg`/`opt`/`flag` as
/// `pavo-driver-kit`'s surface layer spells them. Wire-only params
/// (`Spec` patches) can't ride argv, so they stay out — surfaces
/// only offer what `run` can pass.
fn param_json(p: &registry::Param) -> Value {
    json!({
        "name": p.name,
        "kind": match p.kind {
            PKind::Pos => "arg",
            PKind::Opt => "opt",
            PKind::Flag => "flag",
        },
        "doc": p.help,
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

/// [`run_op`] with the verb already resolved — the `cli.<verb>`
/// wire-query path lands here too.
pub fn run_verb(v: &Verb, params: &Map<String, Value>) -> Response {
    let args = argv(v, params);
    let out = std::env::current_exe().and_then(|exe| {
        std::process::Command::new(exe)
            .arg("run")
            .args(&args)
            .current_dir(workdir(params))
            .output()
    });
    let data = match out {
        Ok(o) => json!({
            "op": v.name,
            "argv": args,
            "code": o.status.code().unwrap_or(-1),
            "stdout": String::from_utf8_lossy(&o.stdout),
            "stderr": String::from_utf8_lossy(&o.stderr),
        }),
        Err(e) => return Response::err(format!("run {}: {e}", v.name)),
    };
    Response::ok_typed(&QueryResult {
        query: format!("cli.{}", v.name),
        data,
    })
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
