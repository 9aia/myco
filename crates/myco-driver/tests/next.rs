//! End-to-end `myco next` — the highest-pri open item (a `view`
//! eval with limit 1), the filter-flag merge, the empty ledger,
//! and `--claim`'s mutation + per-verb commit — driving the real
//! binary against tempdir git repos.
//!
//! `MYCO_CONFIG` is pinned at a nonexistent path so a stray user
//! config can't leak `project_dir` into discovery.

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
        .env_remove("MYCO_ACTOR")
        .output()
        .unwrap()
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

#[test]
fn next_picks_highest_pri_then_id() {
    let (d, _wt) = project();
    run(d.path(), &["todo", "add", "a low task", "--pri", "low"]);
    run(d.path(), &["todo", "add", "a med task"]);
    run(d.path(), &["todo", "add", "a crit task", "--pri", "crit"]);

    let out = run(d.path(), &["next"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let s = stdout(&out);
    assert!(s.starts_with("T-003 open pri:crit a crit task"), "{s}");
    assert_eq!(s.trim().lines().count(), 1, "one line: {s}");

    // --json is the item object under `item`
    let out = run(d.path(), &["next", "--json"]);
    let v = json(&out);
    assert_eq!(v["item"]["id"], "T-003");
    assert_eq!(v["item"]["status"], "open");

    // same pri → lowest id wins; done crit drops out of `open`
    run(d.path(), &["todo", "done", "T-003"]);
    let out = run(d.path(), &["next", "--json"]);
    assert_eq!(json(&out)["item"]["id"], "T-002");

    // issues compete with tasks on the default kind filter
    run(d.path(), &["issue", "new", "a crit issue", "--pri", "crit"]);
    let out = run(d.path(), &["next", "--json"]);
    assert_eq!(json(&out)["item"]["id"], "I-001");
}

#[test]
fn next_filter_flags_merge_over_the_default() {
    let (d, _wt) = project();
    run(d.path(), &["todo", "add", "bug task", "--label", "bug"]);
    run(d.path(), &["todo", "add", "second task"]);
    run(d.path(), &["issue", "new", "plain issue", "--pri", "crit"]);

    // --kind narrows the [task, issue] default
    let out = run(d.path(), &["next", "--kind", "task", "--json"]);
    assert_eq!(json(&out)["item"]["id"], "T-001");
    // --label filters within it
    let out = run(d.path(), &["next", "--label", "bug", "--json"]);
    assert_eq!(json(&out)["item"]["id"], "T-001");
    // --sort overrides pri,id — id-descending takes the high id
    // (kind-then-number order; `=` form since the key is
    // `-`-prefixed, same as `view`)
    let out = run(d.path(), &["next", "--sort=-id", "--json"]);
    assert_eq!(json(&out)["item"]["id"], "I-001");
    // --status replaces the open default — claimed items surface
    run(
        d.path(),
        &[
            "item", "set", "T-001", "--status", "claimed", "--owner", "devin",
        ],
    );
    let out = run(d.path(), &["next", "--status", "claimed", "--json"]);
    assert_eq!(json(&out)["item"]["id"], "T-001");
}

#[test]
fn next_empty_backlog_is_clear() {
    let (d, _wt) = project();
    let out = run(d.path(), &["next"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert!(
        stdout(&out).contains("nothing next — backlog is clear"),
        "{}",
        stdout(&out)
    );
    let out = run(d.path(), &["next", "--json"]);
    assert!(json(&out)["item"].is_null());
    // a filter that matches nothing is the same clear, not an error
    run(d.path(), &["todo", "add", "t"]);
    let out = run(d.path(), &["next", "--kind", "issue", "--json"]);
    assert_eq!(code(&out), 0);
    assert!(json(&out)["item"].is_null());
}

#[test]
fn next_claim_mutates_and_commits() {
    let (d, wt) = project();
    run(d.path(), &["todo", "add", "pick me"]);

    // default claimer is the resolved actor — `human:<git
    // user.name>` here (the fixture repo sets `user.name = t`)
    let out = run(d.path(), &["next", "--claim"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let s = stdout(&out);
    assert!(
        s.contains("T-001 claimed pri:med owner:human:t pick me"),
        "{s}"
    );
    assert_eq!(
        git(&wt, &["log", "-1", "--format=%s"]),
        "chore: claim T-001"
    );
    // on disk: status + owner in TODO.md
    let todo = std::fs::read_to_string(wt.join("TODO.md")).unwrap();
    assert!(
        todo.contains("owner:human:t") || todo.contains("claimed"),
        "{todo}"
    );

    // --owner names the claimer (it doesn't filter under
    // --claim — T-001 is claimed now, so the next open item
    // is claimed as devin)
    run(d.path(), &["todo", "add", "next one"]);
    let out = run(d.path(), &["next", "--claim", "--owner", "devin", "--json"]);
    let v = json(&out);
    assert_eq!(v["item"]["id"], "T-002");
    assert_eq!(v["item"]["owner"], "devin");
    assert_eq!(v["item"]["status"], "claimed");
    assert_eq!(v["committed"], true);
    assert_eq!(
        git(&wt, &["log", "-1", "--format=%s"]),
        "chore: claim T-002"
    );

    // several --owners can't name one claimer
    let out = run(
        d.path(),
        &["next", "--claim", "--owner", "devin", "--owner", "cli"],
    );
    assert_eq!(code(&out), 2);
}

#[test]
fn next_claim_respects_ownership_and_steal() {
    let (d, wt) = project();
    run(d.path(), &["todo", "add", "contested"]);
    // devin owns the open item (owner can be set while open)
    run(d.path(), &["item", "set", "T-001", "--owner", "devin"]);

    // --claim by another actor refuses — the ownership guard
    // fires on the open→claimed transition
    let out = run(d.path(), &["next", "--claim", "--owner", "agent-x"]);
    assert_eq!(code(&out), 2, "{out:?}");
    assert!(stderr(&out).contains("owned by devin"), "{}", stderr(&out));

    // --steal takes it: owner flips, `taken_from` reports
    let out = run(
        d.path(),
        &["next", "--claim", "--owner", "agent-x", "--steal", "--json"],
    );
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let v = json(&out);
    assert_eq!(v["item"]["owner"], "agent-x");
    assert_eq!(v["item"]["status"], "claimed");
    assert_eq!(v["taken_from"], "devin");
    assert!(std::fs::read_to_string(wt.join("handoffs/T-001.md"))
        .unwrap()
        .contains("taken over from devin"));

    // $MYCO_ACTOR is the claimer when --owner is absent
    run(d.path(), &["todo", "add", "env claimed"]);
    let out = myco_cmd()
        .current_dir(d.path())
        .args(["next", "--claim", "--json"])
        .env("MYCO_CONFIG", "/nonexistent/myco.toml")
        .env("MYCO_ACTOR", "agent-env")
        .env_remove("MYCO_NO_COMMIT")
        .output()
        .unwrap();
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let v = json(&out);
    assert_eq!(v["item"]["owner"], "agent-env");
}

#[test]
fn next_claim_honors_no_commit() {
    let (d, wt) = project();
    run(d.path(), &["todo", "add", "pick me"]);
    let head = git(&wt, &["rev-parse", "HEAD"]);
    let out = run(d.path(), &["next", "--claim", "--no-commit", "--json"]);
    let v = json(&out);
    assert_eq!(v["item"]["status"], "claimed");
    assert_eq!(v["committed"], false);
    assert_eq!(git(&wt, &["rev-parse", "HEAD"]), head);
}

#[test]
fn next_marks_interrupted_items() {
    let (d, _wt) = project();
    run(d.path(), &["todo", "add", "was working", "--pri", "crit"]);
    run(d.path(), &["todo", "add", "normal"]);
    // work it, then interrupt — it reverts to open with the
    // interrupted handoff marker
    run(
        d.path(),
        &[
            "item", "set", "T-001", "--status", "claimed", "--owner", "devin",
        ],
    );
    run(
        d.path(),
        &["item", "set", "T-001", "--status", "in-progress"],
    );
    run(d.path(), &["interrupt", "T-001"]);
    let out = run(d.path(), &["next"]);
    assert!(stdout(&out).trim_end().ends_with('⚡'), "{}", stdout(&out));
}
