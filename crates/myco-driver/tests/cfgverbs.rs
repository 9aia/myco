//! End-to-end: `[verbs]` config-declared verbs (P-007) — a
//! view-emit verb through the CLI and through batch NDJSON, an
//! alias verb, builtin-collision warnings, `initialize`'s
//! advertised method table, and unknown-name errors.

#![allow(clippy::unwrap_used)]

use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;

/// Spawn the engine through the driver's `run` passthrough —
/// `tamias-driver-myco run <myco argv>` ≡ the retired `myco` bin
/// (same path the bins[] `cli` shim and `cli.*` wire queries use).
fn myco_cmd() -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_tamias-driver-myco"));
    c.arg("run");
    c
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}");
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

fn run(dir: &Path, args: &[&str]) -> Output {
    myco_cmd()
        .current_dir(dir)
        .args(args)
        .env("MYCO_CONFIG", "/nonexistent/myco.toml")
        .env_remove("MYCO_NO_COMMIT")
        .output()
        .unwrap()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).to_string()
}

fn json(out: &Output) -> Value {
    serde_json::from_slice(&out.stdout)
        .unwrap_or_else(|e| panic!("bad json ({}): {}", e, String::from_utf8_lossy(&out.stdout)))
}

/// A project with two tasks + a `[verbs]` section.
fn fixture() -> tempfile::TempDir {
    let d = repo();
    assert!(run(d.path(), &["init"]).status.success());
    std::env::set_var("MYCO_NO_COMMIT", "1");
    assert!(run(d.path(), &["todo", "add", "first task", "--no-commit"])
        .status
        .success());
    assert!(
        run(d.path(), &["todo", "add", "second task", "--no-commit"])
            .status
            .success()
    );
    std::env::remove_var("MYCO_NO_COMMIT");
    std::fs::write(
        d.path().join("myco.toml"),
        r#"
[verbs.hot]
emit = "first"
filter = { status = ["open"] }

[verbs.standup]
target = "view"
args = ["open"]

[verbs.status]
target = "report"
"#,
    )
    .unwrap();
    d
}

#[test]
fn view_emit_first_via_cli() {
    let d = fixture();
    let out = run(d.path(), &["hot"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(String::from_utf8_lossy(&out.stdout).contains("T-001"));
    // --json parity
    let out = run(d.path(), &["hot", "--json"]);
    let v = json(&out);
    assert_eq!(v["item"]["id"], "T-001");
    assert_eq!(v["view"], "hot");
}

#[test]
fn alias_appends_extra_args_via_cli_and_batch() {
    let d = fixture();
    // CLI — baked ["open"], extra --json appended
    let out = run(d.path(), &["standup", "--json"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let v = json(&out);
    assert_eq!(v["view"], "open");
    assert_eq!(v["groups"].as_array().unwrap().len(), 1);

    // batch NDJSON — the same verb through the served surface
    let mut child = myco_cmd()
        .current_dir(d.path())
        .args(["batch"])
        .env("MYCO_CONFIG", "/nonexistent/myco.toml")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    use std::io::Write;
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(
            b"{\"id\":1,\"method\":\"hot\",\"params\":{}}\n{\"id\":2,\"method\":\"standup\",\"params\":{}}\n",
        )
        .unwrap();
    let out = child.wait_with_output().unwrap();
    let lines: Vec<Value> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines[0]["result"]["item"]["id"], "T-001");
    assert_eq!(lines[1]["result"]["view"], "open");
}

#[test]
fn initialize_advertises_config_verbs_and_builtins_win() {
    let d = fixture();
    let mut child = myco_cmd()
        .current_dir(d.path())
        .args(["batch"])
        .env("MYCO_CONFIG", "/nonexistent/myco.toml")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    use std::io::Write;
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b"{\"id\":1,\"method\":\"initialize\",\"params\":{}}\n{\"id\":2,\"method\":\"nope\",\"params\":{}}\n")
        .unwrap();
    let out = child.wait_with_output().unwrap();
    let lines: Vec<Value> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let methods: Vec<&str> = lines[0]["result"]["methods"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m.as_str().unwrap())
        .collect();
    for m in ["hot", "standup", "status", "view"] {
        assert!(methods.contains(&m), "{m} not in {methods:?}");
    }
    // unknown → -32601
    assert_eq!(lines[1]["error"]["code"], -32601);

    // the [verbs.status] alias collides with the builtin — it
    // warns and the builtin wins, on both surfaces
    let out = run(d.path(), &["status", "--prompt"]);
    assert!(out.status.success());
    assert!(
        stderr(&out).contains("shadows a built-in"),
        "{}",
        stderr(&out)
    );
    assert!(String::from_utf8_lossy(&out.stdout).contains("tasks 2o"));

    let out = run(d.path(), &["nope"]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("unknown verb"), "{}", stderr(&out));
}

#[test]
fn bad_config_verb_warns_and_is_skipped() {
    let d = repo();
    assert!(run(d.path(), &["init"]).status.success());
    std::fs::write(
        d.path().join("myco.toml"),
        "[verbs.bogus]\ntarget = \"nosuch\"\n\n[verbs.ok]\nemit = \"ids\"\n",
    )
    .unwrap();
    // the bad alias is skipped (warned); the good verb still runs
    let out = run(d.path(), &["bogus"]);
    assert!(!out.status.success());
    assert!(
        stderr(&out).contains("unknown alias target"),
        "{}",
        stderr(&out)
    );
    let out = run(d.path(), &["ok", "--json"]);
    assert!(out.status.success(), "{}", stderr(&out));
    // and check lints the repo layer's [verbs]
    let out = run(d.path(), &["check"]);
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("verb-invalid"),
        "stderr: {}",
        stderr(&out)
    );
}
