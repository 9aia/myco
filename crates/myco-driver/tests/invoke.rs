//! End-to-end `invoke`/`submit` — the pavo interaction protocol's
//! action ops. Spawns the driver's own `serve` wire (ndjson on
//! stdio) against a tempdir ledger and checks dispatch, the
//! target→params merge, required-param errors, and the manifest's
//! declared actions/menus/param schemas.

#![allow(clippy::unwrap_used)]

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{json, Value};

const BIN: &str = env!("CARGO_BIN_EXE_tamias-driver-myco");

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?} in {}: {}",
        dir.display(),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

fn repo() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    git(d.path(), &["init", "-q"]);
    git(d.path(), &["config", "user.email", "t@t"]);
    git(d.path(), &["config", "user.name", "t"]);
    std::fs::write(d.path().join("README.md"), "x").unwrap();
    git(d.path(), &["add", "-A"]);
    git(d.path(), &["commit", "-qm", "c0"]);
    d
}

/// `myco init` through the `run` passthrough — sets up `project/`.
fn init(d: &Path) {
    let out = Command::new(BIN)
        .arg("run")
        .arg("init")
        .arg("--json")
        .current_dir(d)
        .env("MYCO_CONFIG", "/nonexistent/myco.toml")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "init failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// A spawned `tamias-driver-myco serve` — the driver's own ndjson
/// wire (not `run serve`, which is the engine's JSON-RPC).
struct Serve {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl Serve {
    fn spawn(dir: &Path) -> Serve {
        let mut child = Command::new(BIN)
            .arg("serve")
            .current_dir(dir)
            .env("MYCO_CONFIG", "/nonexistent/myco.toml")
            .env_remove("MYCO_NO_COMMIT")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        Serve {
            stdin: child.stdin.take().unwrap(),
            stdout: BufReader::new(child.stdout.take().unwrap()),
            child,
        }
    }

    /// One request → one response line.
    fn call(&mut self, req: Value) -> Value {
        writeln!(self.stdin, "{req}").unwrap();
        self.stdin.flush().unwrap();
        let mut line = String::new();
        self.stdout.read_line(&mut line).unwrap();
        serde_json::from_str(line.trim()).unwrap()
    }

    fn invoke(&mut self, action: &str, target: Option<Value>, params: Value) -> Value {
        let mut req = json!({"op": "invoke", "action": action, "params": params});
        if let Some(t) = target {
            req["target"] = t;
        }
        self.call(req)
    }
}

impl Drop for Serve {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// `manifest` argv → parsed DriverManifest JSON.
fn manifest() -> Value {
    let out = Command::new(BIN).arg("manifest").output().unwrap();
    assert!(out.status.success());
    serde_json::from_slice(&out.stdout).unwrap()
}

/// `ops` argv → parsed ops manifest JSON.
fn ops_manifest() -> Value {
    let out = Command::new(BIN).arg("ops").output().unwrap();
    assert!(out.status.success());
    serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).unwrap()
}

fn op_params<'a>(ops: &'a Value, query: &str) -> &'a [Value] {
    ops["ops"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["query"] == json!(query))
        .unwrap_or_else(|| panic!("{query} missing from ops manifest"))["params"]
        .as_array()
        .unwrap()
}

#[test]
fn manifest_declares_actions_menus_and_schemas() {
    let m = manifest();
    let actions = m["actions"].as_array().unwrap();
    let by_id = |id: &str| {
        actions
            .iter()
            .find(|a| a["id"] == json!(id))
            .unwrap_or_else(|| panic!("action {id} missing"))
    };
    for id in [
        "todo.add",
        "todo.done",
        "todo.cancel",
        "todo.defer",
        "backlog.promote",
        "item.set",
        "item.move",
        "item.reorder",
        "undo",
        "redo",
        "handoff",
        "next",
    ] {
        by_id(id);
    }
    // Danger mapping: reads/navigation → none, mutations → confirm,
    // terminal/destructive → type-target.
    assert_eq!(by_id("todo.add")["danger"], json!("none"));
    assert_eq!(by_id("next")["danger"], json!("none"));
    assert_eq!(by_id("item.reorder")["danger"], json!("none"));
    assert_eq!(by_id("todo.done")["danger"], json!("confirm"));
    assert_eq!(by_id("item.set")["danger"], json!("confirm"));
    assert_eq!(by_id("undo")["danger"], json!("confirm"));
    assert_eq!(by_id("redo")["danger"], json!("confirm"));
    assert_eq!(by_id("todo.cancel")["danger"], json!("type-target"));
    // Param schemas on the declared actions.
    let add = by_id("todo.add");
    let title = add["params"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == json!("title"))
        .unwrap();
    assert_eq!(title["kind"], json!("text"));
    assert_eq!(title["required"], json!(true));
    let pri = add["params"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == json!("pri"))
        .unwrap();
    assert_eq!(pri["kind"], json!("enum"));
    assert!(pri["options"].as_array().unwrap().contains(&json!("high")));
    let done = by_id("todo.done");
    assert_eq!(done["params"][0]["kind"], json!("ref"));
    assert_eq!(done["params"][0]["ref_query"], json!("items"));
    // item.reorder — ref id + integer delta, both required.
    let re = by_id("item.reorder");
    let re_params = re["params"].as_array().unwrap();
    let delta = re_params
        .iter()
        .find(|p| p["name"] == json!("delta"))
        .unwrap();
    assert_eq!(delta["kind"], json!("integer"));
    assert_eq!(delta["required"], json!(true));
    // contributes.menus — palette entries pointing at actions/views.
    let menus = m["contributes"]["menus"].as_array().unwrap();
    assert!(!menus.is_empty());
    assert!(menus.iter().any(|e| e["action"] == json!("todo.add")));
    assert!(menus.iter().any(|e| e["action"] == json!("item.reorder")));
    assert!(menus.iter().any(|e| e["action"] == json!("undo")));
    assert!(menus.iter().any(|e| e["action"] == json!("redo")));
    assert!(menus.iter().any(|e| e["route"] == json!("/myco/board")));
}

#[test]
fn ops_params_carry_typed_schemas() {
    let ops = ops_manifest();
    let params = op_params(&ops, "cli.todo.add");
    let title = params.iter().find(|p| p["name"] == json!("title")).unwrap();
    assert_eq!(title["schema"]["kind"], json!("text"));
    assert_eq!(title["schema"]["required"], json!(true));
    let pri = params.iter().find(|p| p["name"] == json!("pri")).unwrap();
    assert_eq!(pri["schema"]["kind"], json!("enum"));
    assert!(pri["schema"]["options"]
        .as_array()
        .unwrap()
        .contains(&json!("crit")));
    // `id` positional → ref picker over the items view.
    let done = op_params(&ops, "cli.todo.done");
    let id = done.iter().find(|p| p["name"] == json!("id")).unwrap();
    assert_eq!(id["schema"]["kind"], json!("ref"));
    assert_eq!(id["schema"]["ref_query"], json!("items"));
    // Status filter → enum over the state machine.
    let list = op_params(&ops, "cli.todo.list");
    let status = list.iter().find(|p| p["name"] == json!("status")).unwrap();
    assert_eq!(status["schema"]["kind"], json!("enum"));
    assert!(status["schema"]["options"]
        .as_array()
        .unwrap()
        .contains(&json!("in-progress")));
}

#[test]
fn invoke_add_done_and_validation() {
    let d = repo();
    init(d.path());
    let mut s = Serve::spawn(d.path());

    // Missing required param → ActionResult.errors, no spawn.
    let r = s.invoke("todo.add", None, json!({}));
    assert_eq!(r["ok"], json!(true));
    assert_eq!(r["result"]["errors"]["title"], json!("required"));

    // Unknown action → wire-level ok:false.
    let r = s.call(json!({"op": "invoke", "action": "bogus.verb", "params": {}}));
    assert_eq!(r["ok"], json!(false));

    // todo.add through invoke — params map straight to argv.
    let r = s.invoke(
        "todo.add",
        None,
        json!({"title": "wire task", "pri": "high", "label": ["wire"]}),
    );
    assert_eq!(r["ok"], json!(true), "{r}");
    assert_eq!(r["result"]["data"]["code"], json!(0), "{r}");
    assert_eq!(r["result"]["data"]["op"], json!("todo.add"));

    // todo.done with an object target — target.id merges into
    // params (the verb's required `id`).
    let r = s.invoke("todo.done", Some(json!({"id": "T-001"})), json!({}));
    assert_eq!(r["ok"], json!(true), "{r}");
    assert_eq!(r["result"]["data"]["code"], json!(0), "{r}");

    // The `cli.`-prefixed spelling resolves too — defer a fresh
    // item added via submit's verb-named adapter below.
    let r = s.invoke("cli.todo.add", None, json!({"title": "second task"}));
    assert_eq!(r["result"]["data"]["code"], json!(0), "{r}");

    // Scalar target fills `id` — the tree-item action shape.
    let r = s.invoke("todo.defer", Some(json!("T-002")), json!({}));
    assert_eq!(r["result"]["data"]["code"], json!(0), "{r}");
}

#[test]
fn submit_dispatches_verb_named_forms() {
    let d = repo();
    init(d.path());
    let mut s = Serve::spawn(d.path());

    // `form` naming a verb → values become the verb params.
    let r = s.call(json!({
        "op": "submit",
        "view": "board",
        "form": "todo.add",
        "values": {"title": "via submit"},
    }));
    assert_eq!(r["ok"], json!(true), "{r}");
    assert_eq!(r["result"]["data"]["op"], json!("todo.add"));

    // A view id that isn't a verb → ok:false (no forms declared).
    let r = s.call(json!({
        "op": "submit",
        "view": "board",
        "values": {},
    }));
    assert_eq!(r["ok"], json!(false));
}

#[test]
fn invoke_reorder_and_undo_redo() {
    let d = repo();
    init(d.path());
    let mut s = Serve::spawn(d.path());
    let todo_md = || std::fs::read_to_string(d.path().join("project/TODO.md")).unwrap();
    let pos = |id: &str| todo_md().find(id).unwrap();

    for title in ["one", "two", "three"] {
        let r = s.invoke("todo.add", None, json!({"title": title}));
        assert_eq!(r["result"]["data"]["code"], json!(0), "{r}");
    }
    assert!(pos("T-001") < pos("T-002") && pos("T-002") < pos("T-003"));

    // item.reorder — delta -2 walks T-003 to the top, one commit.
    // Missing `delta` → ActionResult.errors before any spawn.
    let r = s.invoke("item.reorder", Some(json!("T-003")), json!({}));
    assert_eq!(r["ok"], json!(true));
    assert_eq!(r["result"]["errors"]["delta"], json!("required"));

    let r = s.invoke(
        "item.reorder",
        Some(json!({"id": "T-003"})),
        json!({"delta": -2}),
    );
    assert_eq!(r["ok"], json!(true), "{r}");
    assert_eq!(r["result"]["data"]["code"], json!(0), "{r}");
    assert_eq!(r["result"]["data"]["op"], json!("item.reorder"));
    assert!(pos("T-003") < pos("T-001") && pos("T-001") < pos("T-002"));

    // undo rewinds the reorder commit; redo restores it. The verbs
    // refuse on a published tip — temp repos have no origin, so
    // the journal path is exercised end to end.
    let r = s.invoke("undo", None, json!({}));
    assert_eq!(r["ok"], json!(true), "{r}");
    assert_eq!(r["result"]["data"]["code"], json!(0), "{r}");
    assert!(pos("T-001") < pos("T-002") && pos("T-002") < pos("T-003"));

    let r = s.invoke("redo", None, json!({}));
    assert_eq!(r["ok"], json!(true), "{r}");
    assert_eq!(r["result"]["data"]["code"], json!(0), "{r}");
    assert!(pos("T-003") < pos("T-001") && pos("T-001") < pos("T-002"));

    // `undo --list` is the read-only arm — reports the redoable
    // stack without mutating.
    let r = s.invoke("undo", None, json!({"list": true}));
    assert_eq!(r["result"]["data"]["code"], json!(0), "{r}");
    assert!(pos("T-003") < pos("T-001"));
}
