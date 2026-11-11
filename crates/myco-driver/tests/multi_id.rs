//! End-to-end multi-id reads (P-004) — `myco resume <ID>…` and
//! `myco item show <ID>…` take 1..n ids in one call: blocks in
//! argument order, `[{id, ok, payload|error}]` under `--json`,
//! per-id failures inline + nonzero exit, single-id shapes
//! byte-identical to the pre-batch verbs.

#![allow(clippy::unwrap_used)]

use std::path::{Path, PathBuf};
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

fn run(dir: &Path, args: &[&str]) -> Output {
    myco_cmd()
        .current_dir(dir)
        .args(args)
        .env("MYCO_CONFIG", "/nonexistent/myco.toml")
        .env_remove("MYCO_NO_COMMIT")
        .output()
        .unwrap()
}

/// `run` + extra env vars — for pinning `MYCO_HOME` somewhere
/// dead when a test wants to prove the index isn't consulted.
fn run_env(dir: &Path, args: &[&str], env: &[(&str, &str)]) -> Output {
    let mut c = myco_cmd();
    c.current_dir(dir)
        .args(args)
        .env("MYCO_CONFIG", "/nonexistent/myco.toml")
        .env_remove("MYCO_NO_COMMIT");
    for (k, v) in env {
        c.env(k, v);
    }
    c.output().unwrap()
}

fn json(out: &Output) -> Value {
    serde_json::from_slice(&out.stdout)
        .unwrap_or_else(|e| panic!("bad json ({}): {}", e, String::from_utf8_lossy(&out.stdout)))
}

fn code(out: &Output) -> i32 {
    out.status.code().unwrap_or(-1)
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).to_string()
}

/// A repo with an initialized project worktree.
fn project() -> (tempfile::TempDir, PathBuf) {
    let d = repo();
    let out = run(d.path(), &["init", "--json"]);
    assert_eq!(code(&out), 0, "init failed: {}", stderr(&out));
    let wt = PathBuf::from(json(&out)["worktree"].as_str().unwrap());
    (d, wt)
}

/// A project with a task, an issue, and a handoff on the issue.
fn seeded() -> tempfile::TempDir {
    let (d, _wt) = project();
    run(d.path(), &["todo", "add", "a task"]);
    run(d.path(), &["issue", "new", "an issue"]);
    run(
        d.path(),
        &["handoff", "I-001", "--note", "Done: x. Next: y."],
    );
    d
}

#[test]
fn item_show_multi_repeats_the_block_in_order() {
    let d = seeded();
    let single_t = stdout(&run(d.path(), &["item", "show", "T-001"]));
    let single_i = stdout(&run(d.path(), &["item", "show", "I-001"]));

    // argument order is preserved (I first) and the multi output
    // is exactly the single-id blocks joined on a blank line —
    // the per-item block stays byte-identical
    let out = run(d.path(), &["item", "show", "I-001", "T-001"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert_eq!(
        stdout(&out),
        format!("{}\n\n{}\n", single_i.trim_end(), single_t.trim_end())
    );
}

#[test]
fn item_show_multi_json_shape_and_partial_failure() {
    let d = seeded();

    // n>1 → the per-id array, argument order
    let out = run(d.path(), &["item", "show", "T-001", "I-001", "--json"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let v = json(&out);
    let arr = v.as_array().unwrap();
    assert_eq!(arr.len(), 2);
    assert_eq!(arr[0]["id"], "T-001");
    assert_eq!(arr[0]["ok"], true);
    assert_eq!(arr[0]["payload"]["location"], "TODO.md");
    assert_eq!(arr[1]["id"], "I-001");
    assert_eq!(
        arr[1]["payload"]["location"],
        "issues/issue-0001-an-issue.md"
    );

    // a bad id reports inline — exit 1, batch not aborted
    let out = run(
        d.path(),
        &["item", "show", "T-001", "P-999", "I-001", "--json"],
    );
    assert_eq!(code(&out), 1, "{}", stderr(&out));
    let arr = json(&out);
    let arr = arr.as_array().unwrap();
    assert_eq!(arr.len(), 3);
    assert_eq!(arr[1]["id"], "P-999");
    assert_eq!(arr[1]["ok"], false);
    assert!(arr[1]["error"].as_str().unwrap().contains("no such item"));
    assert_eq!(arr[2]["ok"], true);

    // human: the bad id reports on stderr, the rest still print
    let out = run(d.path(), &["item", "show", "P-999", "T-001"]);
    assert_eq!(code(&out), 1);
    assert!(
        stderr(&out).contains("P-999: no such item"),
        "{}",
        stderr(&out)
    );
    assert!(stdout(&out).contains("T-001"), "{}", stdout(&out));
}

#[test]
fn item_show_related_section_human_and_json() {
    let d = seeded();
    // a real edge: the issue declares it blocks the task
    let out = run(d.path(), &["item", "set", "I-001", "--blocks", "T-001"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));

    // human — a kind-grouped section, titles resolved, the
    // handoff self-edge rendered as its file path
    let out = run(d.path(), &["item", "show", "I-001"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("related:"), "{text}");
    assert!(text.contains("handoff"), "{text}");
    assert!(text.contains("handoffs/I-001.md"), "{text}");
    assert!(text.contains("T-001 a task"), "{text}");

    // the incoming side mirrors the kind — the task is
    // blocked_by the issue
    let out = run(d.path(), &["item", "show", "T-001"]);
    let text = stdout(&out);
    assert!(text.contains("blocked_by"), "{text}");
    assert!(text.contains("I-001 an issue"), "{text}");

    // --json — `{kind: [{id, title?}]}`; MYCO_HOME pointed at
    // nothing to prove the related section never consults the
    // persisted index (it's derived live from the ledger)
    let out = run_env(
        d.path(),
        &["item", "show", "I-001", "--json"],
        &[("MYCO_HOME", "/nonexistent/myco-home")],
    );
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let v = json(&out);
    assert_eq!(
        v["related"]["handoff"],
        serde_json::json!([{"id": "I-001", "title": "an issue"}])
    );
    assert_eq!(
        v["related"]["blocks"],
        serde_json::json!([{"id": "T-001", "title": "a task"}])
    );

    // multi-id shares the one graph — both payloads carry related
    let out = run(d.path(), &["item", "show", "I-001", "T-001", "--json"]);
    let arr = json(&out);
    assert_eq!(arr[0]["payload"]["related"]["blocks"][0]["id"], "T-001");
    assert_eq!(arr[1]["payload"]["related"]["blocked_by"][0]["id"], "I-001");
}

#[test]
fn item_show_single_id_shape_unchanged() {
    let d = seeded();
    // n=1 --json → the bare payload object, not the array
    let out = run(d.path(), &["item", "show", "I-001", "--json"]);
    assert_eq!(code(&out), 0);
    let v = json(&out);
    assert!(v.is_object(), "not the batch array: {v}");
    assert_eq!(v["item"]["id"], "I-001");
    assert!(v.as_object().unwrap().contains_key("handoffs"));
    // n=1 bad id → the classic fatal error + exit 2
    let out = run(d.path(), &["item", "show", "P-999"]);
    assert_eq!(code(&out), 2);
    let out = run(d.path(), &["item", "show", "P-999", "--json"]);
    assert_eq!(code(&out), 2);
    assert!(json(&out)["error"]
        .as_str()
        .unwrap()
        .contains("no such item"));
    // no id at all → clap usage error
    let out = run(d.path(), &["item", "show"]);
    assert_eq!(code(&out), 2);
}

#[test]
fn resume_multi_repeats_the_block_in_order() {
    let d = seeded();
    let out = run(d.path(), &["resume", "I-001", "T-001"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let s = stdout(&out);
    let i_pos = s.find("I-001").unwrap();
    let t_pos = s.find("T-001").unwrap();
    assert!(i_pos < t_pos, "argument order: {s}");
    assert!(s.contains("\n\n"), "blank line separates: {s}");
    // each block carries its own context
    assert!(s.contains("in: issues/issue-0001-an-issue.md"), "{s}");
    assert!(s.contains("Done: x. Next: y."), "{s}");
    assert!(s.contains("in: TODO.md"), "{s}");
}

#[test]
fn resume_multi_json_shape_and_partial_failure() {
    let d = seeded();
    let out = run(d.path(), &["resume", "T-001", "I-001", "--json"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let arr = json(&out);
    let arr = arr.as_array().unwrap();
    assert_eq!(arr.len(), 2);
    assert_eq!(arr[0]["id"], "T-001");
    assert_eq!(arr[0]["ok"], true);
    assert_eq!(arr[0]["payload"]["item"]["id"], "T-001");
    assert_eq!(arr[1]["payload"]["item"]["id"], "I-001");
    assert_eq!(arr[1]["payload"]["handoffs_total"], 1);

    // partial failure: inline + exit 1
    let out = run(d.path(), &["resume", "T-001", "P-999", "--json"]);
    assert_eq!(code(&out), 1);
    let arr = json(&out);
    assert_eq!(arr[1]["id"], "P-999");
    assert_eq!(arr[1]["ok"], false);
    assert!(arr[1]["error"].as_str().unwrap().contains("no such item"));

    // single id keeps the bare payload shape + fatal errors
    let out = run(d.path(), &["resume", "I-001", "--json"]);
    let v = json(&out);
    assert!(v.is_object());
    assert_eq!(v["item"]["id"], "I-001");
    assert!(v.as_object().unwrap().contains_key("handoffs_total"));
    let out = run(d.path(), &["resume", "P-999"]);
    assert_eq!(code(&out), 2);
}
