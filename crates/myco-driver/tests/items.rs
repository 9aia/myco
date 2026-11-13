//! End-to-end item-verb tests — `todo`, `backlog`, `issue`,
//! `proposal`, `plan`, `adr`, `item show|set` driving the real
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

fn read(wt: &Path, rel: &str) -> String {
    std::fs::read_to_string(wt.join(rel)).unwrap()
}

// ---------- todo ----------

#[test]
fn todo_add_list_done() {
    let (d, wt) = project();
    let out = run(
        d.path(),
        &[
            "todo",
            "add",
            "write phase 4",
            "--pri",
            "high",
            "--owner",
            "devin",
            "--label",
            "core",
            "--label",
            "cli",
            "--blocks",
            "T-002",
            "--json",
        ],
    );
    assert_eq!(code(&out), 0);
    let v = json(&out);
    assert_eq!(v["item"]["id"], "T-001");
    assert_eq!(v["item"]["priority"], "high");
    assert_eq!(v["item"]["owner"], "devin");
    assert_eq!(v["item"]["labels"][0], "cli"); // sorted
    assert_eq!(v["item"]["blocks"][0], "T-002");
    assert_eq!(v["committed"], true);
    let todo = read(&wt, "TODO.md");
    assert!(
        todo.contains("- [ ] T-001 write phase 4 · pri:high · owner:devin"),
        "{todo}"
    );
    assert!(todo.contains("#cli · #core"), "{todo}");
    assert_eq!(git(&wt, &["log", "-1", "--format=%s"]), "chore: add T-001");

    // --section lands under the heading (created when absent)
    let out = run(
        d.path(),
        &["todo", "add", "later thing", "--section", "Later"],
    );
    assert_eq!(code(&out), 0);
    let todo = read(&wt, "TODO.md");
    assert!(
        todo.contains("## Later\n\n- [ ] T-002 later thing"),
        "{todo}"
    );

    // filters
    let out = run(d.path(), &["todo", "list", "--owner", "devin", "--json"]);
    let v = json(&out);
    assert_eq!(v.as_array().unwrap().len(), 1);
    let out = run(d.path(), &["todo", "list", "--label", "infra", "--json"]);
    assert_eq!(json(&out).as_array().unwrap().len(), 0);

    // done removes the line (T-001 carries the dangling blocks ref
    // with it); sections that empty out are dropped
    for id in ["T-001", "T-002"] {
        let out = run(d.path(), &["todo", "done", id, "--json"]);
        assert_eq!(code(&out), 0);
        assert_eq!(json(&out)["item"]["status"], "done");
    }
    let todo = read(&wt, "TODO.md");
    assert!(!todo.contains("T-00"), "{todo}");
    // emptied named sections stay — the `##` heading is user
    // structure, not item data (I-021)
    assert!(todo.contains("## Later"), "{todo}");
    assert_eq!(git(&wt, &["log", "-1", "--format=%s"]), "chore: done T-002");

    // id never reused
    let out = run(d.path(), &["todo", "add", "fresh", "--json"]);
    assert_eq!(json(&out)["item"]["id"], "T-003");
    let out = run(d.path(), &["check"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
}

#[test]
fn commits_carry_actor_trailer() {
    let (d, wt) = project();
    // `--owner` names the actor on add/new verbs
    run(d.path(), &["todo", "add", "owned task", "--owner", "devin"]);
    let body = git(&wt, &["log", "-1", "--format=%B"]);
    assert!(body.contains("Actor: devin"), "{body}");
    // subject line is untouched — `%s` parses never see the trailer
    assert_eq!(git(&wt, &["log", "-1", "--format=%s"]), "chore: add T-001");

    // no actor flag → the resolved chain: `human:<git user.name>`
    // (the fixture repo sets `user.name = t`)
    run(d.path(), &["todo", "done", "T-001"]);
    let body = git(&wt, &["log", "-1", "--format=%B"]);
    assert!(body.contains("Actor: human:t"), "{body}");

    // $MYCO_ACTOR outranks the git rung
    let out = myco_cmd()
        .current_dir(d.path())
        .args(["todo", "add", "env actor"])
        .env("MYCO_CONFIG", "/nonexistent/myco.toml")
        .env("MYCO_ACTOR", "agent-x")
        .output()
        .unwrap();
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let body = git(&wt, &["log", "-1", "--format=%B"]);
    assert!(body.contains("Actor: agent-x"), "{body}");

    // `--actor` on continuity verbs
    run(d.path(), &["todo", "add", "another"]);
    let out = run(
        d.path(),
        &[
            "handoff",
            "T-002",
            "--note",
            "Done: x",
            "--actor",
            "human:luis",
        ],
    );
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let body = git(&wt, &["log", "-1", "--format=%B"]);
    assert!(body.contains("Actor: human:luis"), "{body}");

    // author/committer stay the repo's git identity, not the actor
    assert_eq!(git(&wt, &["log", "-1", "--format=%an"]), "t");
    assert_eq!(git(&wt, &["log", "-1", "--format=%ae"]), "t@t");
}

#[test]
fn todo_defer_and_backlog_promote() {
    let (d, wt) = project();
    run(d.path(), &["todo", "add", "task one"]);
    let out = run(d.path(), &["todo", "defer", "T-001", "--json"]);
    assert_eq!(code(&out), 0);
    assert_eq!(json(&out)["item"]["status"], "deferred");
    let todo = read(&wt, "TODO.md");
    assert!(!todo.contains("T-001"), "{todo}");
    assert!(read(&wt, "BACKLOG.md").contains("- T-001 task one"));
    assert_eq!(
        git(&wt, &["log", "-1", "--format=%s"]),
        "chore: defer T-001"
    );

    // defer again → already deferred
    let out = run(d.path(), &["todo", "defer", "T-001"]);
    assert_eq!(code(&out), 2);
    assert!(stderr(&out).contains("already deferred"));

    // done on a backlog item → machine has no deferred→done path
    let out = run(d.path(), &["todo", "done", "T-001"]);
    assert_eq!(code(&out), 2);
    assert!(stderr(&out).contains("promote"), "{}", stderr(&out));

    // promote back
    let out = run(d.path(), &["backlog", "promote", "T-001", "--json"]);
    assert_eq!(code(&out), 0);
    assert_eq!(json(&out)["item"]["status"], "open");
    assert!(read(&wt, "TODO.md").contains("T-001"));
    assert!(!read(&wt, "BACKLOG.md").contains("T-001"));
    // promote twice → not in backlog
    let out = run(d.path(), &["backlog", "promote", "T-001"]);
    assert_eq!(code(&out), 2);
}

#[test]
fn todo_cancel_archives_as_cancelled() {
    let (d, wt) = project();
    run(d.path(), &["todo", "add", "task one"]);
    run(d.path(), &["todo", "add", "task two"]);
    // give task two a child so the subtree archives together
    let out = run(
        d.path(),
        &["todo", "add", "child", "--parent", "T-002", "--json"],
    );
    assert_eq!(code(&out), 0, "{}", stderr(&out));

    let out = run(d.path(), &["todo", "cancel", "T-002", "--json"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert_eq!(json(&out)["item"]["status"], "cancelled");
    assert_eq!(json(&out)["file"], "DONE.md");
    assert!(!read(&wt, "TODO.md").contains("T-002"));
    let done = read(&wt, "DONE.md");
    assert!(
        done.contains("- T-002 task two · status:cancelled"),
        "{done}"
    );
    // the child archives cancelled with its parent
    assert!(done.contains("status:cancelled"), "{done}");
    assert_eq!(
        git(&wt, &["log", "-1", "--format=%s"]),
        "chore: cancel T-002"
    );

    // cancel again → terminal, no transition out
    let out = run(d.path(), &["todo", "cancel", "T-002"]);
    assert_eq!(code(&out), 2);
    assert!(stderr(&out).contains("no such task"), "{}", stderr(&out));

    // `done list` shows the archive — cancelled keeps its status
    let out = run(
        d.path(),
        &["done", "list", "--status", "cancelled", "--json"],
    );
    let rows = json(&out);
    assert_eq!(rows.as_array().unwrap().len(), 2, "{rows}"); // parent + child

    // cancel works straight from BACKLOG.md — no promote needed
    run(d.path(), &["todo", "defer", "T-001"]);
    let out = run(d.path(), &["todo", "cancel", "T-001", "--json"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert!(!read(&wt, "BACKLOG.md").contains("T-001"));
    assert!(read(&wt, "DONE.md").contains("T-001"));

    // `item set --status cancelled` lands in the same archive
    // (T-003 is the child added above; "third" allocs T-004)
    run(d.path(), &["todo", "add", "third"]);
    let out = run(
        d.path(),
        &["item", "set", "T-004", "--status", "cancelled", "--json"],
    );
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert!(read(&wt, "DONE.md").contains("T-004"));
    // and cancelled is terminal — the archived item refuses mutation
    let out = run(d.path(), &["item", "set", "T-004", "--status", "open"]);
    assert_eq!(code(&out), 2);
}

#[test]
fn backlog_add_and_list() {
    let (d, wt) = project();
    let out = run(
        d.path(),
        &[
            "backlog",
            "add",
            "someday idea",
            "--label",
            "maybe",
            "--json",
        ],
    );
    assert_eq!(code(&out), 0);
    let v = json(&out);
    assert_eq!(v["item"]["status"], "deferred");
    assert_eq!(v["file"], "BACKLOG.md");
    let backlog = read(&wt, "BACKLOG.md");
    assert!(backlog.contains("- T-001 someday idea"), "{backlog}");
    assert!(backlog.contains("#maybe"), "{backlog}");

    let out = run(d.path(), &["backlog", "list"]);
    assert!(stdout(&out).contains("T-001 deferred pri:med #maybe someday idea"));
    let out = run(
        d.path(),
        &["backlog", "list", "--status", "deferred", "--json"],
    );
    assert_eq!(json(&out).as_array().unwrap().len(), 1);
    let out = run(d.path(), &["backlog", "list", "--status", "open", "--json"]);
    assert_eq!(json(&out).as_array().unwrap().len(), 0);
}

#[test]
fn todo_list_human_snapshot() {
    let (d, _wt) = project();
    run(d.path(), &["todo", "add", "alpha", "--pri", "high"]);
    run(
        d.path(),
        &["todo", "add", "beta", "--owner", "devin", "--label", "x"],
    );
    let out = run(d.path(), &["todo", "list"]);
    assert_eq!(code(&out), 0);
    insta::assert_snapshot!(stdout(&out));
}

#[test]
fn nested_tasks_lifecycle() {
    let (d, wt) = project();
    // --parent nests the new item under an existing task
    run(d.path(), &["todo", "add", "parent task"]);
    let out = run(
        d.path(),
        &["todo", "add", "sub one", "--parent", "T-001", "--json"],
    );
    assert_eq!(code(&out), 0);
    assert_eq!(json(&out)["parent"], "T-001");
    let out = run(d.path(), &["todo", "add", "sub two", "--parent", "T-001"]);
    assert_eq!(code(&out), 0);
    let todo = read(&wt, "TODO.md");
    for frag in [
        "- [ ] T-001 parent task",
        "  - [ ] T-002 sub one",
        "  - [ ] T-003 sub two",
    ] {
        assert!(todo.contains(frag), "{frag} in {todo}");
    }
    // --section and --parent conflict
    let out = run(
        d.path(),
        &["todo", "add", "x", "--parent", "T-001", "--section", "s"],
    );
    assert_eq!(code(&out), 2);
    // a missing or non-task parent is a clean error
    let out = run(d.path(), &["todo", "add", "x", "--parent", "T-099"]);
    assert_eq!(code(&out), 2);
    run(d.path(), &["issue", "new", "an issue"]);
    let out = run(d.path(), &["todo", "add", "x", "--parent", "I-001"]);
    assert_eq!(code(&out), 2);

    // list indents children under their parent; --json nests them
    let out = run(d.path(), &["todo", "list"]);
    let text = stdout(&out);
    let lines: Vec<&str> = text.lines().collect();
    assert!(
        lines.iter().any(|l| l.starts_with("  T-002")),
        "{}",
        lines.join("\n")
    );
    let out = run(d.path(), &["todo", "list", "--json"]);
    let v = json(&out);
    assert!(
        v.as_array().unwrap().iter().any(|i| i["id"] == "T-002"),
        "{v}"
    );

    // item set reaches children
    let out = run(
        d.path(),
        &["item", "set", "T-002", "--status", "in-progress", "--json"],
    );
    assert_eq!(code(&out), 0);
    let todo = read(&wt, "TODO.md");
    assert!(
        todo.contains("  - [ ] T-002 sub one · status:in-progress"),
        "{todo}"
    );
    // item show finds a child id
    let out = run(d.path(), &["item", "show", "T-003", "--json"]);
    assert_eq!(json(&out)["location"], "TODO.md");

    // done on a child removes just the child — parent + sibling stay
    let out = run(d.path(), &["todo", "done", "T-002", "--json"]);
    assert_eq!(code(&out), 0);
    let todo = read(&wt, "TODO.md");
    assert!(todo.contains("T-001"), "{todo}");
    assert!(todo.contains("T-003"), "{todo}");
    assert!(!todo.contains("T-002"), "{todo}");

    // defer on a parent moves the whole subtree to BACKLOG.md
    let out = run(d.path(), &["todo", "defer", "T-001", "--json"]);
    assert_eq!(code(&out), 0);
    let todo = read(&wt, "TODO.md");
    assert!(!todo.contains("T-00"), "{todo}");
    let backlog = read(&wt, "BACKLOG.md");
    for frag in ["- T-001 parent task", "  - T-003 sub two"] {
        assert!(backlog.contains(frag), "{frag} in {backlog}");
    }
    let out2 = run(d.path(), &["backlog", "list"]);
    assert!(
        stdout(&out2).contains("  T-003"),
        "child indented in backlog list"
    );
    // children park with the parent — no status token needed
    assert!(!backlog.contains("status:"), "{backlog}");

    // a child added under a backlog parent is born deferred
    let out = run(d.path(), &["todo", "add", "late sub", "--parent", "T-001"]);
    assert_eq!(code(&out), 0);
    let backlog = read(&wt, "BACKLOG.md");
    assert!(backlog.contains("  - T-004 late sub"), "{backlog}");

    // promote restores the subtree — children unpark to open
    let out = run(d.path(), &["backlog", "promote", "T-001", "--json"]);
    assert_eq!(code(&out), 0);
    let todo = read(&wt, "TODO.md");
    for frag in [
        "- [ ] T-001 parent task",
        "  - [ ] T-003 sub two",
        "  - [ ] T-004 late sub",
    ] {
        assert!(todo.contains(frag), "{frag} in {todo}");
    }
    assert!(!todo.contains("status:"), "{todo}");

    // done on a parent removes the whole subtree
    let out = run(d.path(), &["todo", "done", "T-001", "--json"]);
    assert_eq!(code(&out), 0);
    let todo = read(&wt, "TODO.md");
    assert!(!todo.contains("T-00"), "{todo}");
    // and the ledger is clean afterwards
    let out = run(d.path(), &["check"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
}

#[test]
fn check_flags_nested_duplicate_and_dangling() {
    let (d, wt) = project();
    let out = run(d.path(), &["check", "--json"]);
    assert_eq!(code(&out), 0);
    std::fs::write(
        wt.join("TODO.md"),
        "# TODO — x\n\n- [ ] T-001 a\n  - [ ] T-002 sub · blocks:T-099\n- [ ] T-002 dup\n",
    )
    .unwrap();
    let out = run(d.path(), &["check", "--json"]);
    assert_eq!(code(&out), 1);
    let v = json(&out);
    let codes: Vec<&str> = v["errors"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["code"].as_str().unwrap())
        .collect();
    assert!(codes.contains(&"duplicate-id"), "{codes:?}");
    assert!(codes.contains(&"dangling-blocks"), "{codes:?}");
}

#[test]
fn check_flags_dangling_links() {
    let (d, wt) = project();
    let out = run(d.path(), &["check", "--json"]);
    assert_eq!(code(&out), 0);
    std::fs::write(
        wt.join("TODO.md"),
        "# TODO — x\n\n- [ ] T-001 a · links:PL-099\n- [ ] T-002 b\n",
    )
    .unwrap();
    let out = run(d.path(), &["check", "--json"]);
    assert_eq!(code(&out), 1);
    let v = json(&out);
    let codes: Vec<&str> = v["errors"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["code"].as_str().unwrap())
        .collect();
    assert!(codes.contains(&"dangling-links"), "{codes:?}");
    // a resolvable link is clean
    run(d.path(), &["plan", "new", "the plan"]);
    std::fs::write(
        wt.join("TODO.md"),
        "# TODO — x\n\n- [ ] T-001 a · links:PL-001\n- [ ] T-002 b\n",
    )
    .unwrap();
    let out = run(d.path(), &["check"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
}

// ---------- issue ----------

#[test]
fn issue_new_list_resolve_archive_comment() {
    let (d, wt) = project();
    let out = run(
        d.path(),
        &[
            "issue",
            "new",
            "index corruption",
            "--pri",
            "crit",
            "--json",
        ],
    );
    assert_eq!(code(&out), 0);
    let v = json(&out);
    assert_eq!(v["item"]["id"], "I-001");
    assert_eq!(v["path"], "issues/issue-0001-index-corruption.md");
    let file = read(&wt, "issues/issue-0001-index-corruption.md");
    for frag in [
        "id = \"I-001\"",
        "status = \"open\"",
        "priority = \"crit\"",
        "## Description",
        "## Checklist",
        "## Resolution",
    ] {
        assert!(file.contains(frag), "{frag} in {file}");
    }
    assert_eq!(
        git(&wt, &["log", "-1", "--format=%s"]),
        "chore: new issue I-001"
    );

    // --text supplies the body
    let out = run(d.path(), &["issue", "new", "b", "--text", "custom body"]);
    assert_eq!(code(&out), 0);
    assert!(read(&wt, "issues/issue-0002-b.md").contains("custom body"));

    // list + filters
    let out = run(d.path(), &["issue", "list"]);
    assert_eq!(stdout(&out).lines().count(), 2);
    let out = run(d.path(), &["issue", "list", "--status", "open", "--json"]);
    assert_eq!(json(&out).as_array().unwrap().len(), 2);

    // comment appends a dated line under ## Comments
    let out = run(d.path(), &["issue", "comment", "I-001", "repro confirmed"]);
    assert_eq!(code(&out), 0);
    let file = read(&wt, "issues/issue-0001-index-corruption.md");
    assert!(file.contains("## Comments\n\n- 20"), "{file}");
    assert!(file.contains("repro confirmed"), "{file}");

    // archive before done → refused
    let out = run(d.path(), &["issue", "archive", "I-001"]);
    assert_eq!(code(&out), 2);
    assert!(stderr(&out).contains("must be done"));

    // resolve walks open → done and appends the resolution
    let out = run(
        d.path(),
        &[
            "issue",
            "resolve",
            "I-001",
            "--resolution",
            "fixed the leak",
            "--json",
        ],
    );
    assert_eq!(code(&out), 0);
    assert_eq!(json(&out)["item"]["status"], "done");
    let file = read(&wt, "issues/issue-0001-index-corruption.md");
    assert!(file.contains("status = \"done\""), "{file}");
    assert!(file.contains("## Resolution\n\nfixed the leak"), "{file}");

    // archive moves the file; list --archived surfaces it
    let out = run(d.path(), &["issue", "archive", "I-001", "--json"]);
    assert_eq!(code(&out), 0);
    assert_eq!(
        json(&out)["archived_to"],
        "issues/archive/issue-0001-index-corruption.md"
    );
    assert!(wt
        .join("issues/archive/issue-0001-index-corruption.md")
        .exists());
    assert!(!wt.join("issues/issue-0001-index-corruption.md").exists());
    let out = run(d.path(), &["issue", "list", "--json"]);
    let v = json(&out);
    let ids: Vec<&str> = v
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|i| i["id"].as_str())
        .collect();
    assert_eq!(ids, ["I-002"]);
    let out = run(d.path(), &["issue", "list", "--archived", "--json"]);
    let v = json(&out);
    let list = v.as_array().unwrap();
    assert_eq!(list.len(), 2);
    // live + archived share one array — "archived" is the marker
    let flag = |id: &str| list.iter().find(|i| i["id"] == id).unwrap()["archived"].as_bool();
    assert_eq!(flag("I-001"), Some(true));
    assert_eq!(flag("I-002"), Some(false));
    // re-archive → error
    let out = run(d.path(), &["issue", "archive", "I-001"]);
    assert_eq!(code(&out), 2);
    // mutating an archived item is refused
    let out = run(d.path(), &["issue", "comment", "I-001", "late note"]);
    assert_eq!(code(&out), 2);
    assert!(stderr(&out).contains("archived"));

    // archived ids stay reserved
    let out = run(d.path(), &["issue", "new", "third", "--json"]);
    assert_eq!(json(&out)["item"]["id"], "I-003");
    // check is clean with the archive dir present
    let out = run(d.path(), &["check"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
}

// ---------- proposal / plan / adr ----------

#[test]
fn proposal_lifecycle() {
    let (d, wt) = project();
    let out = run(d.path(), &["proposal", "new", "the big idea", "--json"]);
    assert_eq!(code(&out), 0);
    assert_eq!(json(&out)["item"]["id"], "P-001");
    let file = read(&wt, "proposals/proposal-0001-the-big-idea.md");
    for frag in [
        "Goal & Overview",
        "Requirements",
        "Technical Design",
        "Open questions",
    ] {
        assert!(file.contains(frag), "{frag} in {file}");
    }

    let out = run(d.path(), &["proposal", "list"]);
    assert!(stdout(&out).contains("P-001 open pri:med the big idea"));

    // status mapping: accepted → done (walked through the machine)
    let out = run(
        d.path(),
        &["proposal", "status", "P-001", "accepted", "--json"],
    );
    assert_eq!(code(&out), 0);
    let v = json(&out);
    assert_eq!(v["to"], "done");
    assert_eq!(v["from"], "open");
    assert!(read(&wt, "proposals/proposal-0001-the-big-idea.md").contains("status = \"done\""));

    // done is terminal — rejecting it now fails
    let out = run(d.path(), &["proposal", "status", "P-001", "rejected"]);
    assert_eq!(code(&out), 2);
    // bogus word
    let out = run(d.path(), &["proposal", "status", "P-001", "lgtm"]);
    assert_eq!(code(&out), 2);
    assert!(stderr(&out).contains("draft|accepted|rejected"));

    // rejected on a fresh proposal works directly
    let out = run(d.path(), &["proposal", "new", "alt"]);
    assert_eq!(code(&out), 0);
    let out = run(d.path(), &["proposal", "status", "P-002", "rejected"]);
    assert_eq!(code(&out), 0);
}

#[test]
fn plan_new_and_list() {
    let (d, wt) = project();
    let out = run(d.path(), &["plan", "new", "v3 rollout", "--pri", "high"]);
    assert_eq!(code(&out), 0);
    let file = read(&wt, "plans/pl-0001-v3-rollout.md");
    assert!(file.contains("id = \"PL-001\""), "{file}");
    assert!(file.contains("- [ ] Phase 1"), "{file}");
    let out = run(d.path(), &["plan", "list", "--json"]);
    let v = json(&out);
    assert_eq!(v.as_array().unwrap()[0]["id"], "PL-001");
    assert_eq!(v.as_array().unwrap()[0]["priority"], "high");
}

#[test]
fn adr_new_and_supersede() {
    let (d, wt) = project();
    run(d.path(), &["adr", "new", "use markdown"]);
    run(d.path(), &["adr", "new", "use yaml instead"]);
    let file = read(&wt, "adrs/0001-use-markdown.md");
    for frag in [
        "## Context",
        "## Decision",
        "## Consequences",
        "Status: **proposed**",
    ] {
        assert!(file.contains(frag), "{frag} in {file}");
    }
    let out = run(d.path(), &["adr", "list"]);
    assert_eq!(stdout(&out).lines().count(), 2);

    let out = run(
        d.path(),
        &["adr", "supersede", "ADR-0001", "--by", "ADR-0002", "--json"],
    );
    assert_eq!(code(&out), 0);
    let v = json(&out);
    assert_eq!(v["superseded"]["superseded_by"], "ADR-0002");
    assert_eq!(v["by"]["supersedes"], "ADR-0001");
    let old = read(&wt, "adrs/0001-use-markdown.md");
    assert!(old.contains("status = \"done\""), "{old}");
    assert!(old.contains("superseded_by = \"ADR-0002\""), "{old}");
    assert!(read(&wt, "adrs/0002-use-yaml-instead.md").contains("supersedes = \"ADR-0001\""));
    assert_eq!(
        git(&wt, &["log", "-1", "--format=%s"]),
        "chore: supersede ADR-0001 by ADR-0002"
    );

    // self-supersede and missing target
    let out = run(
        d.path(),
        &["adr", "supersede", "ADR-0002", "--by", "ADR-0002"],
    );
    assert_eq!(code(&out), 2);
    let out = run(
        d.path(),
        &["adr", "supersede", "ADR-0002", "--by", "ADR-0099"],
    );
    assert_eq!(code(&out), 2);
    // superseding a done (superseded) ADR again → terminal
    let out = run(
        d.path(),
        &["adr", "supersede", "ADR-0001", "--by", "ADR-0002"],
    );
    assert_eq!(code(&out), 2);
}

// ---------- item show / set ----------

#[test]
fn item_show_finds_everything() {
    let (d, _wt) = project();
    run(d.path(), &["todo", "add", "a task"]);
    run(d.path(), &["issue", "new", "an issue"]);
    let out = run(d.path(), &["item", "show", "T-001", "--json"]);
    assert_eq!(code(&out), 0);
    let v = json(&out);
    assert_eq!(v["location"], "TODO.md");
    assert!(v["body"].is_null());
    let out = run(d.path(), &["item", "show", "I-001", "--json"]);
    let v = json(&out);
    assert_eq!(v["location"], "issues/issue-0001-an-issue.md");
    assert!(v["body"].as_str().unwrap().contains("## Description"));
    // human output prints body
    let out = run(d.path(), &["item", "show", "I-001"]);
    assert!(stdout(&out).contains("## Description"));
    // missing → error
    let out = run(d.path(), &["item", "show", "P-009"]);
    assert_eq!(code(&out), 2);
    // archived items still show
    run(d.path(), &["issue", "resolve", "I-001"]);
    run(d.path(), &["issue", "archive", "I-001"]);
    let out = run(d.path(), &["item", "show", "I-001", "--json"]);
    assert_eq!(
        json(&out)["location"],
        "issues/archive/issue-0001-an-issue.md"
    );
}

#[test]
fn item_set_semantics() {
    let (d, wt) = project();
    run(d.path(), &["todo", "add", "a task"]);

    // labels add/remove via +/- and --unlabel
    let out = run(
        d.path(),
        &[
            "item", "set", "T-001", "--label", "+a", "--label", "b", "--json",
        ],
    );
    assert_eq!(code(&out), 0);
    let out = run(
        d.path(),
        &[
            "item",
            "set",
            "T-001",
            "--label",
            "-a",
            "--unlabel",
            "b",
            "--label",
            "c",
        ],
    );
    assert_eq!(code(&out), 0);
    let line = read(&wt, "TODO.md");
    assert!(line.contains("#c"), "{line}");
    assert!(!line.contains("#a"), "{line}");

    // blocks add/remove
    run(d.path(), &["item", "set", "T-001", "--blocks", "+I-001"]);
    let out = run(
        d.path(),
        &["item", "set", "T-001", "--unblock", "I-001", "--json"],
    );
    assert!(json(&out)["item"]["blocks"].is_null());
    let out = run(d.path(), &["item", "set", "T-001", "--blocks", "bogus"]);
    assert_eq!(code(&out), 2);

    // links add/remove — signed values and --unlink
    run(d.path(), &["issue", "new", "a bug"]);
    run(d.path(), &["proposal", "new", "an idea"]);
    run(d.path(), &["item", "set", "T-001", "--links", "+P-001"]);
    let out = run(
        d.path(),
        &["item", "set", "T-001", "--links", "I-001", "--json"],
    );
    assert_eq!(code(&out), 0);
    let v = json(&out);
    let links = v["item"]["links"].as_array().unwrap();
    assert_eq!(links.len(), 2, "{links:?}");
    let out = run(
        d.path(),
        &["item", "set", "T-001", "--unlink", "P-001", "--json"],
    );
    assert_eq!(code(&out), 0);
    let v = json(&out);
    let links = v["item"]["links"].as_array().unwrap();
    assert_eq!(links.len(), 1);
    assert_eq!(links[0], "I-001");
    let line = read(&wt, "TODO.md");
    assert!(line.contains("links:I-001"), "{line}");
    let out = run(d.path(), &["item", "set", "T-001", "--links", "bogus"]);
    assert_eq!(code(&out), 2);

    // pri + owner
    let out = run(
        d.path(),
        &[
            "item", "set", "T-001", "--pri", "crit", "--owner", "devin", "--json",
        ],
    );
    let v = json(&out);
    assert_eq!(v["item"]["priority"], "critical");
    assert_eq!(v["item"]["owner"], "devin");

    // invalid transition → error names the valid targets
    let out = run(d.path(), &["item", "set", "T-001", "--status", "done"]);
    assert_eq!(code(&out), 2);
    let e = stderr(&out);
    assert!(e.contains("open → done"), "{e}");
    assert!(e.contains("claimed, in-progress, deferred"), "{e}");

    // one step at a time: open → in-progress → done removes the line
    run(
        d.path(),
        &["item", "set", "T-001", "--status", "in-progress"],
    );
    let out = run(d.path(), &["item", "set", "T-001", "--status", "done"]);
    assert_eq!(code(&out), 0);
    assert!(!read(&wt, "TODO.md").contains("T-001"));

    // deferred status moves the line to BACKLOG.md
    run(d.path(), &["todo", "add", "move me"]);
    let out = run(d.path(), &["item", "set", "T-002", "--status", "deferred"]);
    assert_eq!(code(&out), 0);
    assert!(!read(&wt, "TODO.md").contains("T-002"));
    assert!(read(&wt, "BACKLOG.md").contains("T-002"));

    // and open moves it back
    let out = run(d.path(), &["item", "set", "T-002", "--status", "open"]);
    assert_eq!(code(&out), 0);
    assert!(read(&wt, "TODO.md").contains("T-002"));

    // noop → usage error
    let out = run(d.path(), &["item", "set", "T-002"]);
    assert_eq!(code(&out), 2);
    assert!(stderr(&out).contains("nothing to change"));

    // file-backed items get the same treatment
    run(d.path(), &["issue", "new", "a bug"]);
    let out = run(
        d.path(),
        &["item", "set", "I-001", "--status", "in-progress", "--json"],
    );
    assert_eq!(code(&out), 0);
    assert!(read(&wt, "issues/issue-0001-a-bug.md").contains("status = \"in-progress\""));
    let out = run(
        d.path(),
        &["item", "set", "I-001", "--label", "+perf", "--json"],
    );
    assert_eq!(json(&out)["item"]["labels"][0], "perf");

    // set on an archived item is refused
    run(d.path(), &["issue", "resolve", "I-001"]);
    run(d.path(), &["issue", "archive", "I-001"]);
    let out = run(d.path(), &["item", "set", "I-001", "--pri", "low"]);
    assert_eq!(code(&out), 2);
    assert!(stderr(&out).contains("archived"));

    // unknown id
    let out = run(d.path(), &["item", "set", "PL-009", "--pri", "low"]);
    assert_eq!(code(&out), 2);
}

#[test]
fn item_set_blocks_accepts_commas() {
    let (d, _wt) = project();
    run(d.path(), &["todo", "add", "a"]);
    run(d.path(), &["todo", "add", "b"]);
    run(d.path(), &["todo", "add", "c"]);
    // comma-separated like `todo add --blocks`
    let out = run(
        d.path(),
        &["item", "set", "T-001", "--blocks", "T-002,T-003", "--json"],
    );
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert_eq!(json(&out)["item"]["blocks"].as_array().unwrap().len(), 2);
    // and --unblock takes commas too
    let out = run(
        d.path(),
        &["item", "set", "T-001", "--unblock", "T-002,T-003", "--json"],
    );
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert!(json(&out)["item"]["blocks"].is_null());
}

#[test]
fn item_set_status_keeps_its_section() {
    let (d, wt) = project();
    // "## Now" holds T-001 alone; "## Later" follows — a status
    // change must not drop T-001 into the unsectioned run (or
    // append a new "Now" at the tail)
    run(d.path(), &["todo", "add", "first", "--section", "Now"]);
    run(d.path(), &["todo", "add", "later", "--section", "Later"]);
    let out = run(
        d.path(),
        &["item", "set", "T-001", "--status", "claimed", "--json"],
    );
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let todo = read(&wt, "TODO.md");
    let now = todo.find("## Now").unwrap();
    let later = todo.find("## Later").unwrap();
    let item = todo.find("T-001").unwrap();
    assert!(now < item && item < later, "{todo}");
    // the emptied-while-held section still exists and there's
    // exactly one "Now" heading
    assert_eq!(todo.matches("## Now").count(), 1, "{todo}");
    assert!(todo.contains("status:claimed"), "{todo}");
    // …and the ledger stays clean
    let out = run(d.path(), &["check"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
}

#[test]
fn item_set_ownership_guard_and_steal() {
    let (d, wt) = project();
    run(d.path(), &["todo", "add", "contested"]);
    // devin claims it
    let out = run(
        d.path(),
        &[
            "item", "set", "T-001", "--status", "claimed", "--owner", "devin", "--json",
        ],
    );
    assert_eq!(code(&out), 0, "{}", stderr(&out));

    // another actor's owner reassignment on a work-state item
    // refuses without --steal
    let out = run(d.path(), &["item", "set", "T-001", "--owner", "agent-x"]);
    assert_eq!(code(&out), 2, "{out:?}");
    assert!(stderr(&out).contains("owned by devin"), "{}", stderr(&out));
    assert!(stderr(&out).contains("--steal"), "{}", stderr(&out));
    // the owner didn't move
    let out = run(d.path(), &["item", "show", "T-001", "--json"]);
    assert_eq!(json(&out)["item"]["owner"], "devin");

    // --steal takes it: owner flips, `taken_from` reports, and a
    // `taken from` handoff entry records the takeover
    let out = run(
        d.path(),
        &[
            "item", "set", "T-001", "--owner", "agent-x", "--steal", "--json",
        ],
    );
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let v = json(&out);
    assert_eq!(v["item"]["owner"], "agent-x");
    assert_eq!(v["taken_from"], "devin");
    let h = read(&wt, "handoffs/T-001.md");
    assert!(h.contains("taken over from devin"), "{h}");

    // the same actor re-assigning its own claim needs no --steal
    let out = run(
        d.path(),
        &["item", "set", "T-001", "--status", "in-progress"],
    );
    assert_eq!(code(&out), 0, "{}", stderr(&out));

    // `--steal` without an ownership conflict is a plain set
    run(d.path(), &["todo", "add", "unclaimed-ish"]);
    let out = run(
        d.path(),
        &[
            "item", "set", "T-002", "--status", "claimed", "--steal", "--json",
        ],
    );
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    // no --owner → the resolved actor (`human:t` — git user.name)
    assert_eq!(json(&out)["item"]["owner"], "human:t");
}

#[test]
fn item_move_cross_repo() {
    // `--to` falls back to registered repo names when the arg
    // isn't a path — pin MYCO_HOME so that lookup never touches
    // the real index.
    let home = tempfile::tempdir().unwrap();
    let run = |dir: &Path, args: &[&str]| {
        myco_cmd()
            .current_dir(dir)
            .args(args)
            .env("MYCO_CONFIG", "/nonexistent/myco.toml")
            .env("MYCO_HOME", home.path())
            .env_remove("MYCO_NO_COMMIT")
            .output()
            .unwrap()
    };
    let (src, src_wt) = project();
    let (dst, dst_wt) = project();
    run(
        src.path(),
        &[
            "todo",
            "add",
            "port this",
            "--pri",
            "high",
            "--label",
            "core",
        ],
    );
    run(src.path(), &["issue", "new", "leaky vault"]);

    // task: line goes, handoff drops to archive with the pointer
    let out = run(
        src.path(),
        &[
            "item",
            "move",
            "T-001",
            "--to",
            dst.path().to_str().unwrap(),
            "--json",
        ],
    );
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let v = json(&out);
    assert_eq!(v["id"], "T-001");
    assert_eq!(v["to"]["id"], "T-001");
    assert_eq!(v["item"]["title"], "port this");
    assert_eq!(v["committed"], true);
    assert_eq!(v["committed_source"], true);
    assert_eq!(v["committed_destination"], true);
    let todo = read(&dst_wt, "TODO.md");
    assert!(todo.contains("- [ ] T-001 port this · pri:high"), "{todo}");
    assert!(todo.contains("#core"), "{todo}");
    assert!(!read(&src_wt, "TODO.md").contains("T-001"));
    let h = read(&src_wt, "handoffs/archive/T-001.md");
    assert!(h.contains("moved to"), "{h}");
    // the destination copy carries the provenance entry too
    let h = read(&dst_wt, "handoffs/T-001.md");
    assert!(h.contains("moved from"), "{h}");

    // file kind: body + meta carry, source closes done in place
    let out = run(
        src.path(),
        &[
            "item",
            "move",
            "I-001",
            "--to",
            dst_wt.to_str().unwrap(), // the worktree path works too
            "--note",
            "per ADR-0001",
            "--json",
        ],
    );
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let v = json(&out);
    assert_eq!(v["id"], "I-001");
    assert_eq!(v["to"]["id"], "I-001");
    let out = run(dst.path(), &["item", "show", "I-001", "--json"]);
    let v = json(&out);
    assert_eq!(v["item"]["title"], "leaky vault");
    assert_eq!(v["item"]["status"], "open");
    let out = run(src.path(), &["item", "show", "I-001", "--json"]);
    assert_eq!(json(&out)["item"]["status"], "done");
    let h = read(&src_wt, "handoffs/I-001.md");
    assert!(h.contains("moved to") && h.contains("per ADR-0001"), "{h}");

    // both ledgers stay check-clean
    for d in [src.path(), dst.path()] {
        let out = run(d, &["check"]);
        assert_eq!(code(&out), 0, "{}: {}", d.display(), stderr(&out));
    }

    // error paths
    run(src.path(), &["todo", "add", "stay"]);
    let out = run(
        src.path(),
        &[
            "item",
            "move",
            "T-002",
            "--to",
            src.path().to_str().unwrap(),
        ],
    );
    assert_eq!(code(&out), 2);
    assert!(stderr(&out).contains("this repo"), "{}", stderr(&out));
    let out = run(
        src.path(),
        &[
            "item",
            "move",
            "T-099",
            "--to",
            dst.path().to_str().unwrap(),
        ],
    );
    assert_eq!(code(&out), 2);
    assert!(stderr(&out).contains("no such item"), "{}", stderr(&out));
    let out = run(
        src.path(),
        &["item", "move", "T-002", "--to", "/nonexistent/repo"],
    );
    assert_eq!(code(&out), 2);
    assert!(stderr(&out).contains("not a myco repo"), "{}", stderr(&out));
    // nothing burned on the failures
    assert!(read(&src_wt, "TODO.md").contains("T-002 stay"));
}

#[test]
fn item_move_dry_run_finish_and_dropped_refs() {
    let home = tempfile::tempdir().unwrap();
    let run = |dir: &Path, args: &[&str]| {
        myco_cmd()
            .current_dir(dir)
            .args(args)
            .env("MYCO_CONFIG", "/nonexistent/myco.toml")
            .env("MYCO_HOME", home.path())
            .env_remove("MYCO_NO_COMMIT")
            .env_remove("MYCO_ACTOR")
            .output()
            .unwrap()
    };
    let (src, src_wt) = project();
    let (dst, dst_wt) = project();
    // T-001 stays; T-002 carries a blocks dep that can't cross —
    // it names a source-ledger id
    run(src.path(), &["todo", "add", "stay behind"]);
    run(
        src.path(),
        &["todo", "add", "port this", "--blocks", "T-001"],
    );

    // --dry-run: the plan reports, nothing is written anywhere.
    // The dest ledger is empty → the copy would land T-001 there.
    let todo_before = read(&src_wt, "TODO.md");
    let out = run(
        src.path(),
        &[
            "item",
            "move",
            "T-002",
            "--to",
            dst.path().to_str().unwrap(),
            "--dry-run",
            "--json",
        ],
    );
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let v = json(&out);
    assert_eq!(v["dry_run"], true);
    assert_eq!(v["to"]["id"], "T-001");
    assert_eq!(v["dropped"], "dropped refs: blocks T-001");
    assert_eq!(read(&src_wt, "TODO.md"), todo_before);
    assert!(!read(&dst_wt, "TODO.md").contains("port this"));
    assert!(!dst_wt.join("handoffs/T-001.md").exists());

    // the real move records the dropped refs in both provenance
    // handoffs
    let out = run(
        src.path(),
        &[
            "item",
            "move",
            "T-002",
            "--to",
            dst.path().to_str().unwrap(),
            "--json",
        ],
    );
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert!(read(&dst_wt, "handoffs/T-001.md").contains("dropped refs: blocks T-001"));
    // the source handoff is archived with the pointer + the drop
    let h = read(&src_wt, "handoffs/archive/T-002.md");
    assert!(
        h.contains("moved to") && h.contains("dropped refs: blocks T-001"),
        "{h}"
    );

    // --finish on a completed move is idempotent — the archived
    // pointer still resolves, the source is already closed
    let out = run(src.path(), &["item", "move", "T-002", "--finish", "--json"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let v = json(&out);
    assert_eq!(v["already_closed"], true);
    assert_eq!(v["to"]["id"], "T-001");

    // --finish on an item that was never moved refuses
    run(src.path(), &["todo", "add", "never moved"]);
    let out = run(src.path(), &["item", "move", "T-003", "--finish"]);
    assert_eq!(code(&out), 2, "{out:?}");
    assert!(
        stderr(&out).contains("no `moved to` handoff"),
        "{}",
        stderr(&out)
    );
    // and --finish rejects a --to
    let out = run(
        src.path(),
        &[
            "item",
            "move",
            "T-002",
            "--finish",
            "--to",
            dst.path().to_str().unwrap(),
        ],
    );
    assert_eq!(code(&out), 2, "{out:?}");

    // a *partial* move finishes: the destination copy exists and
    // the `moved to` pointer is on file, but the source line is
    // still open — simulate the crash state by hand (the pointer
    // names the dst repo root; its T-001 plays the copy)
    let (src2, src2_wt) = project();
    let (dst2, _dst2_wt) = project();
    run(src2.path(), &["todo", "add", "half moved"]);
    run(dst2.path(), &["todo", "add", "dest seed"]);
    let out = run(
        src2.path(),
        &[
            "handoff",
            "T-001",
            "--note",
            &format!("moved to {}:T-001", dst2.path().display()),
        ],
    );
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let out = run(
        src2.path(),
        &["item", "move", "T-001", "--finish", "--json"],
    );
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let v = json(&out);
    assert_eq!(v["already_closed"], false);
    assert!(!read(&src2_wt, "TODO.md").contains("half moved"));
}

#[test]
fn kind_locks_and_missing_ids() {
    let (d, _wt) = project();
    run(d.path(), &["todo", "add", "t"]);
    run(d.path(), &["issue", "new", "i"]);
    // a task id to the issue verbs → usage error, not lookup
    let out = run(d.path(), &["issue", "resolve", "T-001"]);
    assert_eq!(code(&out), 2);
    assert!(stderr(&out).contains("I-NNN"), "{}", stderr(&out));
    let out = run(d.path(), &["todo", "done", "I-001"]);
    assert_eq!(code(&out), 2);
    let out = run(d.path(), &["todo", "done", "T-099"]);
    assert_eq!(code(&out), 2);
    assert!(stderr(&out).contains("no such task"));
}

#[test]
fn issue_body_from_file_and_stdin() {
    let (d, wt) = project();
    std::fs::write(d.path().join("body.md"), "# from file\n\nfull body\n").unwrap();
    let out = run(
        d.path(),
        &[
            "issue",
            "new",
            "from file",
            "--body",
            d.path().join("body.md").to_str().unwrap(),
        ],
    );
    assert_eq!(code(&out), 0);
    assert!(read(&wt, "issues/issue-0001-from-file.md").contains("full body"));

    // --body - reads stdin
    let out = myco_cmd()
        .current_dir(d.path())
        .args(["issue", "new", "piped", "--body", "-"])
        .env("MYCO_CONFIG", "/nonexistent/myco.toml")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut c| {
            use std::io::Write;
            c.stdin.as_mut().unwrap().write_all(b"piped body")?;
            c.wait_with_output()
        })
        .unwrap();
    assert_eq!(code(&out), 0);
    assert!(read(&wt, "issues/issue-0002-piped.md").contains("piped body"));

    // --body and --text conflict (clap)
    let out = run(
        d.path(),
        &["issue", "new", "x", "--body", "f", "--text", "y"],
    );
    assert_eq!(code(&out), 2);
    // missing body file → error
    let out = run(d.path(), &["issue", "new", "x", "--body", "/nope.md"]);
    assert_eq!(code(&out), 2);
}

// ---------- DONE.md / filters / undo ----------

#[test]
fn done_archives_the_subtree_into_done_md() {
    let (d, wt) = project();
    run(d.path(), &["todo", "add", "parent", "--pri", "high"]);
    run(d.path(), &["todo", "add", "child", "--parent", "T-001"]);
    // done on the parent archives the whole subtree
    let out = run(d.path(), &["todo", "done", "T-001", "--json"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let todo = read(&wt, "TODO.md");
    assert!(!todo.contains("T-00"), "{todo}");
    let done = read(&wt, "DONE.md");
    assert!(done.contains("- T-001 parent"), "{done}");
    // the child rode along, nested under its parent — both done
    assert!(done.contains("  - T-002 child"), "{done}");
    // one commit carried the whole mutation
    assert_eq!(git(&wt, &["log", "-1", "--format=%s"]), "chore: done T-001");

    // done list reads the archive — children flatten like
    // `todo list`; item show still resolves each
    let out = run(d.path(), &["done", "list", "--json"]);
    let v = json(&out);
    let ids: Vec<&str> = v
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["T-001", "T-002"], "{v}");
    let out = run(d.path(), &["item", "show", "T-002", "--json"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert_eq!(json(&out)["location"], "DONE.md");
    assert_eq!(json(&out)["item"]["status"], "done");

    // check is clean — the archive is canonical ledger state
    let out = run(d.path(), &["check"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
}

#[test]
fn list_filters_cover_pri_epic_milestone_query() {
    let (d, _wt) = project();
    run(
        d.path(),
        &[
            "todo",
            "add",
            "alpha task",
            "--pri",
            "high",
            "--epic",
            "v2",
            "--milestone",
            "m1",
            "--label",
            "core",
        ],
    );
    run(d.path(), &["todo", "add", "beta task", "--pri", "low"]);
    run(d.path(), &["todo", "add", "gamma", "--epic", "v2"]);

    let titles = |args: &[&str]| -> Vec<String> {
        let mut a = vec!["todo", "list", "--json"];
        a.extend_from_slice(args);
        let v = json(&run(d.path(), &a));
        v.as_array()
            .unwrap()
            .iter()
            .map(|i| i["title"].as_str().unwrap().to_string())
            .collect()
    };
    assert_eq!(titles(&["--pri", "high"]), ["alpha task"]);
    assert_eq!(titles(&["--epic", "v2"]).len(), 2);
    assert_eq!(titles(&["--milestone", "m1"]), ["alpha task"]);
    // query is case-insensitive over id/title/labels
    assert_eq!(titles(&["--query", "ALPHA"]), ["alpha task"]);
    assert_eq!(titles(&["--query", "t-001"]), ["alpha task"]);
    assert_eq!(titles(&["--query", "core"]), ["alpha task"]);
    // filters compose
    assert_eq!(
        titles(&["--epic", "v2", "--pri", "low"]),
        Vec::<String>::new()
    );
    // the archive filters the same way
    run(d.path(), &["todo", "done", "T-002"]);
    let out = run(d.path(), &["done", "list", "--query", "beta", "--json"]);
    assert_eq!(json(&out).as_array().unwrap().len(), 1);
    let out = run(d.path(), &["done", "list", "--status", "open", "--json"]);
    assert_eq!(json(&out).as_array().unwrap().len(), 0);
}

#[test]
fn undo_rewinds_the_last_verb() {
    let (d, wt) = project();
    run(d.path(), &["todo", "add", "temporary"]);
    assert!(read(&wt, "TODO.md").contains("T-001"));
    let tip = git(&wt, &["log", "-1", "--format=%s"]);
    assert_eq!(tip, "chore: add T-001");

    let out = run(d.path(), &["undo", "--json"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let v = json(&out);
    assert_eq!(v["undone"]["subject"], "chore: add T-001");
    // the mutation is gone — file state AND history tip
    assert!(!read(&wt, "TODO.md").contains("T-001"));
    assert_eq!(
        git(&wt, &["log", "-1", "--format=%s"]),
        "chore: init project ledger"
    );

    // keep undoing — init's own commits unwind too; the orphan
    // root is where it stops
    let mut last = String::new();
    for _ in 0..4 {
        let out = run(d.path(), &["undo"]);
        if code(&out) != 0 {
            last = stderr(&out);
            break;
        }
    }
    assert!(last.contains("root commit"), "{last}");
}

#[test]
fn item_reorder_swaps_siblings() {
    let (d, wt) = project();
    run(d.path(), &["todo", "add", "one"]);
    run(d.path(), &["todo", "add", "two"]);
    run(d.path(), &["todo", "add", "three"]);
    let todo = read(&wt, "TODO.md");
    let pos = |t: &str| todo.find(t).unwrap();
    assert!(pos("T-001") < pos("T-002") && pos("T-002") < pos("T-003"));

    // delta -2 walks T-003 to the top — one commit, TUI parity
    // (`Alt+↑` twice).
    let out = run(
        d.path(),
        &["item", "reorder", "T-003", "--delta", "-2", "--json"],
    );
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let v = json(&out);
    assert_eq!(v["file"], "TODO.md");
    assert_eq!(v["moved"], -2);
    assert_eq!(v["committed"], true);
    assert_eq!(
        git(&wt, &["log", "-1", "--format=%s"]),
        "chore: reorder T-003"
    );
    let todo = read(&wt, "TODO.md");
    let pos = |t: &str| todo.find(t).unwrap();
    assert!(
        pos("T-003") < pos("T-001") && pos("T-001") < pos("T-002"),
        "{todo}"
    );

    // the top edge refuses — no commit, no write
    let out = run(d.path(), &["item", "reorder", "T-003", "--delta", "-1"]);
    assert_eq!(code(&out), 2);
    assert!(
        stderr(&out).contains("already at the top"),
        "{}",
        stderr(&out)
    );
    // delta 0 and a file-backed id are usage/lookup errors
    let out = run(d.path(), &["item", "reorder", "T-001", "--delta", "0"]);
    assert_eq!(code(&out), 2);
    assert!(stderr(&out).contains("nonzero"), "{}", stderr(&out));
    run(d.path(), &["issue", "new", "filed", "--body", "b"]);
    let out = run(d.path(), &["item", "reorder", "I-001", "--delta", "1"]);
    assert_eq!(code(&out), 2);
    assert!(stderr(&out).contains("takes T-NNN"), "{}", stderr(&out));

    // nested children swap among their own siblings only
    run(d.path(), &["todo", "add", "parent"]);
    run(d.path(), &["todo", "add", "kid-a", "--parent", "T-004"]);
    run(d.path(), &["todo", "add", "kid-b", "--parent", "T-004"]);
    let out = run(
        d.path(),
        &["item", "reorder", "T-006", "--delta", "-1", "--json"],
    );
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let todo = read(&wt, "TODO.md");
    let (a, b) = (todo.find("T-005").unwrap(), todo.find("T-006").unwrap());
    assert!(b < a, "{todo}");
    // …and a child can't climb out of its parent — delta -9
    // stops inside the sibling list (T-006 is now first child)
    let out = run(d.path(), &["item", "reorder", "T-006", "--delta", "-9"]);
    assert_eq!(code(&out), 2);
    assert!(
        stderr(&out).contains("already at the top"),
        "{}",
        stderr(&out)
    );

    // sections bound the move — an item alone in `## Later`
    // can't reach the unsectioned run above it
    run(d.path(), &["todo", "add", "parked", "--section", "Later"]);
    let out = run(d.path(), &["item", "reorder", "T-007", "--delta", "-5"]);
    assert_eq!(code(&out), 2);
    assert!(
        stderr(&out).contains("already at the top"),
        "{}",
        stderr(&out)
    );

    // BACKLOG.md reorders the same way
    run(d.path(), &["backlog", "add", "b1"]);
    run(d.path(), &["backlog", "add", "b2"]);
    let out = run(
        d.path(),
        &["item", "reorder", "T-009", "--delta", "-1", "--json"],
    );
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert_eq!(json(&out)["file"], "BACKLOG.md");
    let backlog = read(&wt, "BACKLOG.md");
    assert!(backlog.find("T-009").unwrap() < backlog.find("T-008").unwrap());
}
