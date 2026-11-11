//! End-to-end continuity tests — `myco handoff` / `myco resume`
//! over `handoffs/<ID>.md`, plus `check`'s orphan/parse handling —
//! driving the engine (`tamias-driver-myco run`) against tempdir git repos.
//!
//! `MYCO_CONFIG` is pinned at a nonexistent path so a stray user
//! config can't leak `project_dir` into discovery.

#![allow(clippy::unwrap_used)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

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

fn run_stdin(dir: &Path, args: &[&str], stdin: &str) -> Output {
    let mut child = myco_cmd()
        .current_dir(dir)
        .args(args)
        .env("MYCO_CONFIG", "/nonexistent/myco.toml")
        .env_remove("MYCO_NO_COMMIT")
        .env_remove("MYCO_ACTOR")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    use std::io::Write;
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
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

/// The `code` fields of a `check --json` error list.
fn codes(out: &Output) -> Vec<String> {
    json(out)["errors"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["code"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn init_scaffolds_handoffs_dir() {
    let (_d, wt) = project();
    assert!(wt.join("handoffs/.gitkeep").exists());
    // an existing project without handoffs/ is repaired by check --fix
    let (d2, wt2) = project();
    std::fs::remove_dir_all(wt2.join("handoffs")).unwrap();
    let out = run(d2.path(), &["check", "--json"]);
    assert_eq!(code(&out), 1);
    let v = json(&out);
    let codes: Vec<&str> = v["errors"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["code"].as_str().unwrap())
        .collect();
    assert!(codes.contains(&"missing-file"), "{codes:?}");
    let out = run(d2.path(), &["check", "--fix"]);
    assert_eq!(code(&out), 0);
    assert!(wt2.join("handoffs/.gitkeep").exists());
}

#[test]
fn handoff_every_kind_and_append_only() {
    let (d, wt) = project();
    // one item of every kind
    run(d.path(), &["todo", "add", "a task"]);
    run(d.path(), &["issue", "new", "an issue"]);
    run(d.path(), &["proposal", "new", "a proposal"]);
    run(d.path(), &["adr", "new", "a decision"]);
    run(d.path(), &["plan", "new", "a plan"]);

    for id in ["T-001", "I-001", "P-001", "ADR-0001", "PL-001"] {
        let out = run(
            d.path(),
            &[
                "handoff",
                id,
                "--note",
                "Done: x. Next: y.",
                "--actor",
                "devin",
                "--session",
                "cli:test",
                "--json",
            ],
        );
        assert_eq!(code(&out), 0, "{id}: {}", stderr(&out));
        let v = json(&out);
        assert_eq!(v["path"], format!("handoffs/{id}.md"));
        assert_eq!(v["entry"]["actor"], "devin");
        assert_eq!(v["entry"]["session"], "cli:test");
        assert_eq!(v["entry"]["note"], "Done: x. Next: y.");
        assert_eq!(v["committed"], true);
        let file = read(&wt, &format!("handoffs/{id}.md"));
        assert!(file.contains(&format!("item = \"{id}\"")), "{file}");
        assert!(file.contains(&format!("# Handoffs — {id}")), "{file}");
        assert!(file.contains(" · devin · session:cli:test"), "{file}");
    }
    assert_eq!(
        git(&wt, &["log", "-1", "--format=%s"]),
        "chore: handoff PL-001"
    );

    // append-only: second entry lands after the first, bytes kept
    let before = read(&wt, "handoffs/T-001.md");
    let out = run(
        d.path(),
        &["handoff", "T-001", "--note", "second note", "--json"],
    );
    assert_eq!(code(&out), 0);
    let v = json(&out);
    // default actor is the resolved chain — `human:<git user.name>`
    // here; no session key
    assert_eq!(v["entry"]["actor"], "human:t");
    assert!(v["entry"].as_object().unwrap().get("session").is_none());
    let after = read(&wt, "handoffs/T-001.md");
    assert!(after.starts_with(&before), "{after}");
    assert!(after.contains("\n## "), "{after}");
    assert!(after.contains("second note"), "{after}");

    // --note - reads stdin
    let out = run_stdin(
        d.path(),
        &["handoff", "T-001", "--note", "-"],
        "stdin note\n",
    );
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert!(read(&wt, "handoffs/T-001.md").contains("stdin note"));
}

#[test]
fn handoff_errors() {
    let (d, _wt) = project();
    // missing item
    let out = run(d.path(), &["handoff", "T-009", "--note", "x"]);
    assert_eq!(code(&out), 2);
    assert!(stderr(&out).contains("no such item"), "{}", stderr(&out));
    // bad session id
    run(d.path(), &["todo", "add", "t"]);
    let out = run(
        d.path(),
        &["handoff", "T-001", "--note", "x", "--session", "a·b"],
    );
    assert_eq!(code(&out), 2);
    assert!(stderr(&out).contains("session"), "{}", stderr(&out));
    // empty note
    let out = run(d.path(), &["handoff", "T-001", "--note", "  "]);
    assert_eq!(code(&out), 2);
    assert!(stderr(&out).contains("empty"), "{}", stderr(&out));
    // a `##` inside the note would corrupt the entry grammar
    let out = run(
        d.path(),
        &["handoff", "T-001", "--note", "a\n\n## 2020-01-01 · x\nb"],
    );
    assert_eq!(code(&out), 2, "{}", stdout(&out));
    // archived items still take handoffs (appended to the live file)
    run(d.path(), &["issue", "new", "i"]);
    run(d.path(), &["issue", "resolve", "I-001"]);
    run(d.path(), &["issue", "archive", "I-001"]);
    let out = run(d.path(), &["handoff", "I-001", "--note", "late note"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert!(d.path().join("project/handoffs/I-001.md").exists());
}

#[test]
fn resume_json_and_prompt_block() {
    let (d, wt) = project();
    run(d.path(), &["todo", "add", "blocker one"]);
    run(d.path(), &["issue", "new", "the bug", "--blocks", "T-001"]);
    run(
        d.path(),
        &[
            "handoff",
            "I-001",
            "--note",
            "Done: repro. Next: fix.",
            "--actor",
            "devin",
            "--session",
            "cli:abc123",
        ],
    );
    let out = run(d.path(), &["resume", "I-001", "--json"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let v = json(&out);
    assert_eq!(v["item"]["id"], "I-001");
    assert_eq!(v["location"], "issues/issue-0001-the-bug.md");
    assert_eq!(
        v["blocks_titles"][0],
        serde_json::json!({"id": "T-001", "title": "blocker one"})
    );
    assert_eq!(v["handoffs"].as_array().unwrap().len(), 1);
    assert_eq!(v["handoffs"][0]["session"], "cli:abc123");
    assert_eq!(v["handoffs_total"], 1);
    // the issue template's `- [ ] Action item` line is open
    assert_eq!(v["open_checklist"][0], "Action item");

    // human output is the compact prompt block
    let out = run(d.path(), &["resume", "I-001"]);
    let s = stdout(&out);
    assert!(s.contains("I-001 open pri:med the bug"), "{s}");
    assert!(s.contains("blocks:"), "{s}");
    assert!(s.contains("T-001 blocker one"), "{s}");
    assert!(s.contains("handoffs:"), "{s}");
    assert!(s.contains("Done: repro. Next: fix."), "{s}");
    assert!(s.contains("open checklist:"), "{s}");

    // dangling blocks report a null title
    run(d.path(), &["issue", "new", "dangling", "--blocks", "T-099"]);
    let out = run(d.path(), &["resume", "I-002", "--json"]);
    assert_eq!(json(&out)["blocks_titles"][0]["title"], Value::Null);

    // T-001 has no handoffs/blocks/checklist → empty sections
    let out = run(d.path(), &["resume", "T-001", "--json"]);
    assert_eq!(code(&out), 0);
    let v = json(&out);
    assert!(v["handoffs"].as_array().unwrap().is_empty());
    assert!(v["blocks_titles"].as_array().unwrap().is_empty());
    assert!(v["open_checklist"].as_array().unwrap().is_empty());
    let out = run(d.path(), &["resume", "T-001"]);
    let s = stdout(&out);
    assert!(!s.contains("handoffs:"), "{s}");

    // missing item errors
    let out = run(d.path(), &["resume", "P-009"]);
    assert_eq!(code(&out), 2);

    // archived items resume too
    run(d.path(), &["issue", "resolve", "I-001"]);
    run(d.path(), &["issue", "archive", "I-001"]);
    let out = run(d.path(), &["resume", "I-001", "--json"]);
    assert_eq!(code(&out), 0);
    assert!(json(&out)["location"].as_str().unwrap().contains("archive"));
    let _ = wt;
}

#[test]
fn resume_last_caps_entries() {
    let (d, _wt) = project();
    run(d.path(), &["todo", "add", "t"]);
    for n in 1..=4 {
        run(
            d.path(),
            &["handoff", "T-001", "--note", &format!("note {n}")],
        );
    }
    let out = run(d.path(), &["resume", "T-001", "--last", "2", "--json"]);
    let v = json(&out);
    let hs = v["handoffs"].as_array().unwrap();
    assert_eq!(hs.len(), 2);
    // latest two, in file order
    assert_eq!(hs[0]["note"], "note 3");
    assert_eq!(hs[1]["note"], "note 4");
    assert_eq!(v["handoffs_total"], 4);
    // default cap is 10 — all 4 shown
    let out = run(d.path(), &["resume", "T-001", "--json"]);
    assert_eq!(json(&out)["handoffs"].as_array().unwrap().len(), 4);
    // human output shows the truncation marker
    let out = run(d.path(), &["resume", "T-001", "--last", "1"]);
    let s = stdout(&out);
    assert!(s.contains("note 4"), "{s}");
    assert!(s.contains("… +3 earlier entries"), "{s}");
}

#[test]
fn check_orphans_and_malformed_handoffs() {
    let (d, wt) = project();
    run(d.path(), &["todo", "add", "t"]);
    run(d.path(), &["handoff", "T-001", "--note", "keep me"]);
    // done archives the task into DONE.md — the item still
    // exists there, so its handoff is NOT an orphan (same rule
    // as archived item files)
    run(
        d.path(),
        &["item", "set", "T-001", "--status", "in-progress"],
    );
    run(d.path(), &["item", "set", "T-001", "--status", "done"]);
    let out = run(d.path(), &["check", "--json"]);
    assert_eq!(code(&out), 0, "{}", stdout(&out));
    assert!(!codes(&out).contains(&"orphan-handoff".into()));
    // a handoff whose item is truly gone IS an orphan
    std::fs::write(
        wt.join("handoffs/T-999.md"),
        "+++\nitem = \"T-999\"\n+++\n\n# Handoffs — T-999\n\n## 2025-01-01 · cli\norphan note\n",
    )
    .unwrap();
    let out = run(d.path(), &["check", "--json"]);
    assert_eq!(code(&out), 1);
    assert!(codes(&out).contains(&"orphan-handoff".into()));
    // --fix moves it to handoffs/archive/ — knowledge kept, dir clean
    let out = run(d.path(), &["check", "--fix"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert!(!wt.join("handoffs/T-999.md").exists());
    let archived = read(&wt, "handoffs/archive/T-999.md");
    assert!(archived.contains("orphan note"), "{archived}");
    // an archived item's handoff is NOT an orphan
    run(d.path(), &["issue", "new", "i"]);
    run(d.path(), &["handoff", "I-001", "--note", "n"]);
    run(d.path(), &["issue", "resolve", "I-001"]);
    run(d.path(), &["issue", "archive", "I-001"]);
    let out = run(d.path(), &["check", "--json"]);
    let found = codes(&out);
    assert!(
        !found.contains(&"orphan-handoff".into()),
        "archived items keep live handoffs: {found:?}"
    );

    // malformed entry heading → parse finding, not fixable
    std::fs::write(
        wt.join("handoffs/I-002.md"),
        "+++\nitem = \"I-002\"\n+++\n\n# Handoffs — I-002\n\n## not a heading\nnote\n",
    )
    .unwrap();
    let out = run(d.path(), &["check", "--json"]);
    assert_eq!(code(&out), 1);
    let v = json(&out);
    let parse_finding = v["errors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["code"] == "parse" && e["file"] == "handoffs/I-002.md");
    assert!(parse_finding.is_some(), "{v}");
    // non-canonical filename gets renamed by --fix (the item must
    // exist, else it's an orphan first)
    run(d.path(), &["todo", "add", "t2"]); // T-002 exists
    std::fs::write(
        wt.join("handoffs/T-2.md"),
        "+++\nitem = \"T-002\"\n+++\n\n# Handoffs — T-002\n",
    )
    .unwrap();
    let out = run(d.path(), &["check", "--json"]);
    assert!(codes(&out).contains(&"filename".into()));
    let out = run(d.path(), &["check", "--fix"]);
    assert!(wt.join("handoffs/T-002.md").exists(), "{}", stderr(&out));
    assert!(!wt.join("handoffs/T-2.md").exists());
    // unexpected junk in the dir
    std::fs::write(wt.join("handoffs/notes.txt"), "x").unwrap();
    let out = run(d.path(), &["check", "--json"]);
    assert!(codes(&out).contains(&"unexpected-file".into()));
}

#[test]
fn item_show_surfaces_handoff_count() {
    let (d, _wt) = project();
    run(d.path(), &["todo", "add", "t"]);
    let out = run(d.path(), &["item", "show", "T-001", "--json"]);
    assert_eq!(json(&out)["handoffs"], 0);
    run(d.path(), &["handoff", "T-001", "--note", "n"]);
    run(d.path(), &["handoff", "T-001", "--note", "n2"]);
    let out = run(d.path(), &["item", "show", "T-001", "--json"]);
    assert_eq!(json(&out)["handoffs"], 2);
    let out = run(d.path(), &["item", "show", "T-001"]);
    assert!(stdout(&out).contains("handoffs: 2"));
}

#[test]
fn interrupt_reverts_status_and_marks_the_handoff() {
    let (d, wt) = project();
    run(d.path(), &["todo", "add", "wip", "--section", "Now"]);
    run(
        d.path(),
        &["item", "set", "T-001", "--status", "in-progress"],
    );
    let out = run(
        d.path(),
        &["interrupt", "T-001", "--note", "ctx died", "--json"],
    );
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let v = json(&out);
    assert_eq!(v["status"], "open");
    assert!(v["entry"]["note"]
        .as_str()
        .unwrap()
        .starts_with("interrupted"));
    // status reverted on disk — and the section survived
    let todo = read(&wt, "TODO.md");
    assert!(todo.contains("## Now"), "{todo}");
    assert!(todo.contains("- [ ] T-001"), "{todo}");
    assert!(!todo.contains("status:in-progress"), "{todo}");
    // the marker handoff landed; item show flags it
    let out = run(d.path(), &["item", "show", "T-001", "--json"]);
    assert_eq!(json(&out)["interrupted"], true);
    let h = read(&wt, "handoffs/T-001.md");
    assert!(h.contains("interrupted — ctx died"), "{h}");
    // one commit covers both writes
    assert_eq!(
        git(&wt, &["log", "-1", "--format=%s"]),
        "chore: interrupt T-001"
    );
    // open items can't be interrupted
    let out = run(d.path(), &["interrupt", "T-001"]);
    assert_eq!(code(&out), 2);
    assert!(stderr(&out).contains("nothing to interrupt"));
}

#[test]
fn session_scoped_claim_and_teardown() {
    let (d, wt) = project();
    run(d.path(), &["todo", "add", "s1 task"]);
    run(d.path(), &["todo", "add", "s2 task"]);
    run(d.path(), &["todo", "add", "sessionless"]);

    // `item set --session` tags the claim — the mutation and the
    // `claimed` handoff entry ride one commit
    let out = run(
        d.path(),
        &[
            "item",
            "set",
            "T-001",
            "--status",
            "in-progress",
            "--session",
            "sepia:s1",
            "--json",
        ],
    );
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let v = json(&out);
    assert_eq!(v["claim"]["session"], "sepia:s1");
    assert_eq!(v["claim"]["note"], "claimed");
    let h = read(&wt, "handoffs/T-001.md");
    assert!(h.contains("session:sepia:s1"), "{h}");
    assert!(h.contains("claimed"), "{h}");
    let out = run(
        d.path(),
        &[
            "item",
            "set",
            "T-002",
            "--status",
            "claimed",
            "--owner",
            "devin",
            "--session",
            "sepia:s2",
        ],
    );
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    // a sessionless claim stays untagged
    run(
        d.path(),
        &["item", "set", "T-003", "--status", "in-progress"],
    );

    // --session scopes the single-id form too: s2 can't kill s1's
    let out = run(d.path(), &["interrupt", "T-001", "--session", "sepia:s2"]);
    assert_eq!(code(&out), 2);
    let e = stderr(&out);
    assert!(e.contains("claimed under session sepia:s1"), "{e}");
    // …and nothing changed
    let todo = read(&wt, "TODO.md");
    assert!(todo.contains("status:in-progress"), "{todo}");

    // the teardown — only s1's items revert, one commit
    let out = run(d.path(), &["interrupt", "--session", "sepia:s1", "--json"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let v = json(&out);
    assert_eq!(v["session"], "sepia:s1");
    let ids: Vec<&str> = v["interrupted"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["T-001"]);
    assert!(v["errors"].as_array().unwrap().is_empty());
    assert_eq!(v["interrupted"][0]["entry"]["session"], "sepia:s1");
    assert_eq!(
        git(&wt, &["log", "-1", "--format=%s"]),
        "chore: interrupt session sepia:s1"
    );
    // T-001 reverted; s2's and the sessionless claim untouched
    let todo = read(&wt, "TODO.md");
    assert!(todo.contains("- [ ] T-001"), "{todo}");
    assert!(todo.contains("status:claimed"), "{todo}");
    assert!(todo.contains("status:in-progress"), "{todo}");
    // the interrupt marker is tagged too — resume shows it
    let h = read(&wt, "handoffs/T-001.md");
    assert!(h.contains("interrupted"), "{h}");

    // a session that claimed nothing tears down to nothing
    let out = run(d.path(), &["interrupt", "--session", "sepia:ghost"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert!(
        stdout(&out).contains("nothing claimed under"),
        "{}",
        stdout(&out)
    );

    // bare interrupt names the two modes
    let out = run(d.path(), &["interrupt"]);
    assert_eq!(code(&out), 2);
    assert!(stderr(&out).contains("pass an item id"), "{}", stderr(&out));

    // s2's teardown lands its own item
    let out = run(d.path(), &["interrupt", "--session", "sepia:s2", "--json"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert_eq!(json(&out)["interrupted"].as_array().unwrap().len(), 1);
}

#[test]
fn next_claim_session_tags_the_claim() {
    let (d, wt) = project();
    run(d.path(), &["todo", "add", "the work", "--pri", "high"]);
    run(d.path(), &["todo", "add", "less urgent", "--pri", "low"]);
    // --session without --claim is meaningless
    let out = run(d.path(), &["next", "--session", "sepia:s9"]);
    assert_eq!(code(&out), 2);
    assert!(stderr(&out).contains("--claim"), "{}", stderr(&out));
    // claim under a session — tag lands in the handoff entry
    let out = run(
        d.path(),
        &[
            "next",
            "--claim",
            "--owner",
            "devin",
            "--session",
            "sepia:s9",
            "--json",
        ],
    );
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert_eq!(json(&out)["item"]["id"], "T-001");
    let h = read(&wt, "handoffs/T-001.md");
    assert!(h.contains("session:sepia:s9"), "{h}");
    assert!(h.contains("claimed"), "{h}");
    // teardown of that session picks it up
    let out = run(d.path(), &["interrupt", "--session", "sepia:s9", "--json"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let v = json(&out);
    assert_eq!(v["interrupted"].as_array().unwrap().len(), 1);
    let todo = read(&wt, "TODO.md");
    assert!(todo.contains("- [ ] T-001"), "{todo}");
}

#[test]
fn session_tag_validation_and_edges() {
    let (d, _wt) = project();
    run(d.path(), &["todo", "add", "t"]);
    // bad session ids are rejected on every tag path
    for args in [
        vec![
            "item",
            "set",
            "T-001",
            "--status",
            "claimed",
            "--session",
            "a·b",
        ],
        vec!["item", "set", "T-001", "--session", "a·b"],
        vec!["interrupt", "--session", "a·b"],
        vec!["next", "--claim", "--session", "a·b"],
    ] {
        let out = run(d.path(), &args);
        assert_eq!(code(&out), 2, "{args:?}: {}", stderr(&out));
        assert!(
            stderr(&out).contains("session"),
            "{args:?}: {}",
            stderr(&out)
        );
    }
    // --session on an open item — there's no claim to tag
    let out = run(d.path(), &["item", "set", "T-001", "--session", "sepia:s1"]);
    assert_eq!(code(&out), 2);
    assert!(stderr(&out).contains("no claim"), "{}", stderr(&out));
    // but it *does* tag an existing claim (e.g. adopt one)
    run(d.path(), &["item", "set", "T-001", "--status", "claimed"]);
    let out = run(d.path(), &["item", "set", "T-001", "--session", "sepia:s1"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    // …so the session teardown finds it
    let out = run(d.path(), &["interrupt", "--session", "sepia:s1", "--json"]);
    assert_eq!(json(&out)["interrupted"].as_array().unwrap().len(), 1);
}

// ---------------------------------------------------------------- reap

/// `N` days before today as `YYYY-MM-DD` — the test clock the
/// reap sweep's idle math reads (`created:`/handoff dates).
fn days_ago(n: i64) -> String {
    let d = time::OffsetDateTime::now_utc().date() - time::Duration::days(n);
    myco_core::date::render(d)
}

/// Replace the 10-char date after `created:`/`created = "` in `s`.
fn set_created_date(s: &str, needle: &str, date: &str) -> String {
    match s.find(needle) {
        Some(pos) => {
            let start = pos + needle.len();
            let end = (start + 10).min(s.len());
            format!("{}{}{}", &s[..start], date, &s[end..])
        }
        None => s.to_string(),
    }
}

/// Backdate the `created:` token on the TODO.md line carrying `id`
/// — the idle clock for handoff-less items.
fn backdate_created(wt: &Path, id: &str, date: &str) {
    let p = wt.join("TODO.md");
    let s = std::fs::read_to_string(&p).unwrap();
    let out = s
        .lines()
        .map(|l| {
            if l.split_whitespace().any(|t| t == id) {
                set_created_date(l, "created:", date)
            } else {
                l.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    let out = if s.ends_with('\n') {
        format!("{out}\n")
    } else {
        out
    };
    std::fs::write(p, out).unwrap();
}

/// Same backdate for a file-backed item — the `created = "…"`
/// frontmatter key (`rel` from `item show --json`'s `location`).
fn backdate_file_created(wt: &Path, rel: &str, date: &str) {
    let p = wt.join(rel);
    let s = std::fs::read_to_string(&p).unwrap();
    std::fs::write(p, set_created_date(&s, "created = \"", date)).unwrap();
}

/// Rewrite every `## YYYY-MM-DD` heading date in a handoffs file —
/// the claim tag's write date feeds the idle clock too.
fn backdate_handoffs(wt: &Path, id: &str, date: &str) {
    let p = wt.join(format!("handoffs/{id}.md"));
    let s = std::fs::read_to_string(&p).unwrap();
    let out = s
        .lines()
        .map(|l| {
            if l.starts_with("## ") && l.len() >= 13 {
                format!("## {}{}", date, &l[13..])
            } else {
                l.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(p, format!("{out}\n")).unwrap();
}

/// The `reaped[].id` list of a reap `--json` payload.
fn reaped_ids(v: &Value) -> Vec<String> {
    v["reaped"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["id"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn reap_sweeps_idle_claims_in_one_commit() {
    let (d, wt) = project();
    run(d.path(), &["todo", "add", "idle wip"]);
    run(d.path(), &["todo", "add", "idle claim"]);
    run(d.path(), &["todo", "add", "fresh wip"]);
    run(d.path(), &["todo", "add", "untouched open"]);
    run(d.path(), &["issue", "new", "idle bug"]);
    run(
        d.path(),
        &["item", "set", "T-001", "--status", "in-progress"],
    );
    run(
        d.path(),
        &[
            "item", "set", "T-002", "--status", "claimed", "--owner", "devin",
        ],
    );
    run(
        d.path(),
        &["item", "set", "T-003", "--status", "in-progress"],
    );
    run(
        d.path(),
        &["item", "set", "I-001", "--status", "in-progress"],
    );
    // everything idle 10d — tasks via the TODO line, the issue via
    // its frontmatter
    let old = days_ago(10);
    for id in ["T-001", "T-002", "T-003", "T-004"] {
        backdate_created(&wt, id, &old);
    }
    let out = run(d.path(), &["item", "show", "I-001", "--json"]);
    let rel = json(&out)["location"].as_str().unwrap().to_string();
    backdate_file_created(&wt, &rel, &old);
    // T-003's latest handoff is today — last activity wins over
    // `created`, so it survives the sweep
    run(d.path(), &["handoff", "T-003", "--note", "still warm"]);

    let before = git(&wt, &["rev-list", "--count", "HEAD"]);
    let out = run(d.path(), &["reap", "--json"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let v = json(&out);
    assert_eq!(v["dry_run"], false);
    assert_eq!(v["committed"], true);
    assert!(v["errors"].as_array().unwrap().is_empty());
    // ledger order — tasks first, file kinds after
    assert_eq!(reaped_ids(&v), ["T-001", "T-002", "I-001"]);
    let e = &v["reaped"][0];
    assert_eq!(e["title"], "idle wip");
    assert_eq!(e["owner"], Value::Null);
    assert_eq!(e["session"], Value::Null);
    assert!(e["idle_days"].as_i64().unwrap() >= 9, "{e}");
    let e = &v["reaped"][1];
    assert_eq!(e["owner"], "devin");

    // ONE commit for the whole sweep
    assert_eq!(
        git(&wt, &["log", "-1", "--format=%s"]),
        "chore: reap 3 stale claims"
    );
    assert_eq!(
        git(&wt, &["rev-list", "--count", "HEAD"]),
        (before.parse::<usize>().unwrap() + 1).to_string()
    );

    // every hit reverted to open + carries the reap marker
    let todo = read(&wt, "TODO.md");
    for id in ["T-001", "T-002"] {
        assert!(todo.contains(&format!("- [ ] {id}")), "{todo}");
    }
    // T-003 (fresh handoff) and T-004 (open) untouched
    assert!(todo.contains("status:in-progress"), "{todo}");
    assert_eq!(todo.matches("status:in-progress").count(), 1, "{todo}");
    assert!(!todo.contains("status:claimed"), "{todo}");
    let h = read(&wt, "handoffs/T-001.md");
    assert!(h.contains("interrupted — reaped after"), "{h}");
    assert!(h.contains("idle days"), "{h}");
    // the issue file reverted too — its handoff got the marker
    let h = read(&wt, "handoffs/I-001.md");
    assert!(h.contains("interrupted — reaped after"), "{h}");
    let out = run(d.path(), &["item", "show", "I-001", "--json"]);
    assert_eq!(json(&out)["item"]["status"], "open");
    assert_eq!(json(&out)["interrupted"], true);

    // a second sweep finds nothing — clean no-op, no commit
    let before = git(&wt, &["rev-list", "--count", "HEAD"]);
    let out = run(d.path(), &["reap", "--json"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let v = json(&out);
    assert_eq!(reaped_ids(&v), Vec::<String>::new());
    assert_eq!(v["committed"], false);
    assert_eq!(git(&wt, &["rev-list", "--count", "HEAD"]), before);
}

#[test]
fn reap_idle_days_session_scope_and_dry_run() {
    let (d, wt) = project();
    run(d.path(), &["todo", "add", "s1 work"]);
    run(d.path(), &["todo", "add", "s2 work"]);
    run(d.path(), &["todo", "add", "loose work"]);
    run(
        d.path(),
        &[
            "item",
            "set",
            "T-001",
            "--status",
            "in-progress",
            "--session",
            "sepia:s1",
        ],
    );
    run(
        d.path(),
        &[
            "item",
            "set",
            "T-002",
            "--status",
            "claimed",
            "--owner",
            "devin",
            "--session",
            "sepia:s2",
        ],
    );
    run(
        d.path(),
        &["item", "set", "T-003", "--status", "in-progress"],
    );
    // everything idle 10d — created AND the claim-tag handoff
    // entries (the tag's write date is the last activity)
    let old = days_ago(10);
    for id in ["T-001", "T-002", "T-003"] {
        backdate_created(&wt, id, &old);
    }
    // the session-tagged claims have handoff entries — their write
    // dates are the last activity too (T-003 has none: sessionless)
    backdate_handoffs(&wt, "T-001", &old);
    backdate_handoffs(&wt, "T-002", &old);

    // --dry-run: lists the sweep, writes nothing
    let before = git(&wt, &["rev-list", "--count", "HEAD"]);
    let out = run(
        d.path(),
        &["reap", "--dry-run", "--session", "sepia:s1", "--json"],
    );
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let v = json(&out);
    assert_eq!(v["dry_run"], true);
    assert_eq!(v["committed"], false);
    assert_eq!(reaped_ids(&v), ["T-001"]);
    assert_eq!(v["reaped"][0]["session"], "sepia:s1");
    assert_eq!(
        git(&wt, &["rev-list", "--count", "HEAD"]),
        before,
        "dry-run committed"
    );
    let todo = read(&wt, "TODO.md");
    assert!(todo.contains("status:in-progress"), "{todo}");

    // --session scopes the real sweep the same way
    let out = run(
        d.path(),
        &[
            "reap",
            "--session",
            "sepia:s1",
            "--idle-days",
            "5",
            "--json",
        ],
    );
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let v = json(&out);
    assert_eq!(reaped_ids(&v), ["T-001"]);
    assert_eq!(v["committed"], true);
    assert_eq!(
        git(&wt, &["log", "-1", "--format=%s"]),
        "chore: reap 1 stale claims"
    );
    // the marker keeps the session tag — resume shows provenance
    let h = read(&wt, "handoffs/T-001.md");
    assert!(h.contains("session:sepia:s1"), "{h}");
    let todo = read(&wt, "TODO.md");
    assert!(todo.contains("status:claimed"), "{todo}"); // T-002
    assert!(todo.contains("status:in-progress"), "{todo}"); // T-003

    // --idle-days respected: 10d idle survives a 20d threshold
    let out = run(d.path(), &["reap", "--idle-days", "20", "--json"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert_eq!(reaped_ids(&json(&out)), Vec::<String>::new());
    assert_eq!(json(&out)["committed"], false);
    // …and a 5d threshold takes the rest, unscoped
    let out = run(d.path(), &["reap", "--idle-days", "5", "--json"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert_eq!(reaped_ids(&json(&out)), ["T-002", "T-003"]);
    let todo = read(&wt, "TODO.md");
    assert!(!todo.contains("status:"), "{todo}");
}
