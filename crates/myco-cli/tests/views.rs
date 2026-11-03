//! End-to-end view tests — `schema`, `status`, `view`, `report`,
//! `index`, `org` driving the real binary against tempdir git
//! repos. `MYCO_HOME` pins the index at a tempdir per test;
//! `MYCO_CONFIG` is pinned nonexistent so a stray user config
//! can't leak into discovery.

#![allow(clippy::unwrap_used)]

use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;

const MYCO: &str = env!("CARGO_BIN_EXE_myco");

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

fn run(dir: &Path, args: &[&str], myco_home: Option<&Path>) -> Output {
    let mut c = Command::new(MYCO);
    c.current_dir(dir)
        .args(args)
        .env("MYCO_CONFIG", "/nonexistent/myco.toml")
        .env_remove("MYCO_NO_COMMIT");
    match myco_home {
        Some(h) => c.env("MYCO_HOME", h),
        None => c.env_remove("MYCO_HOME"),
    };
    c.output().unwrap()
}

fn json(out: &Output) -> Value {
    serde_json::from_slice(&out.stdout)
        .unwrap_or_else(|e| panic!("bad json ({}): {}", e, String::from_utf8_lossy(&out.stdout)))
}

fn code(out: &Output) -> i32 {
    out.status.code().unwrap_or(-1)
}

fn init(d: &Path) {
    let out = run(d, &["init", "--json"], None);
    assert_eq!(code(&out), 0, "init failed: {out:?}");
}

fn ok(d: &Path, args: &[&str]) -> Output {
    let out = run(d, args, None);
    assert_eq!(code(&out), 0, "{args:?} failed: {out:?}");
    out
}

/// `run` with extra env overrides — config/HOME pinning beyond
/// the shared `MYCO_CONFIG=/nonexistent` baseline.
fn run_env(dir: &Path, args: &[&str], myco_home: Option<&Path>, envs: &[(&str, &str)]) -> Output {
    let mut c = Command::new(MYCO);
    c.current_dir(dir)
        .args(args)
        .env("MYCO_CONFIG", "/nonexistent/myco.toml")
        .env_remove("MYCO_NO_COMMIT");
    match myco_home {
        Some(h) => c.env("MYCO_HOME", h),
        None => c.env_remove("MYCO_HOME"),
    };
    for (k, v) in envs {
        c.env(k, v);
    }
    c.output().unwrap()
}

/// A repo at `dir` with init + a mixed ledger (name = dir basename).
fn seeded_at(dir: &Path) {
    std::fs::create_dir_all(dir).unwrap();
    git(dir, &["init", "-q"]);
    git(dir, &["config", "user.email", "t@t"]);
    git(dir, &["config", "user.name", "t"]);
    std::fs::write(dir.join("README.md"), "x").unwrap();
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-qm", "c0"]);
    init(dir);
    ok(dir, &["todo", "add", "plain task"]);
    ok(dir, &["todo", "add", "hot task", "--pri", "high"]);
    ok(dir, &["issue", "new", "a bug", "--owner", "devin"]);
    ok(dir, &["backlog", "add", "someday idea"]);
}

/// A repo with init + a mixed ledger (name = dir basename).
fn seeded() -> tempfile::TempDir {
    let d = repo();
    init(d.path());
    ok(d.path(), &["todo", "add", "plain task"]);
    ok(d.path(), &["todo", "add", "hot task", "--pri", "high"]);
    ok(d.path(), &["issue", "new", "a bug", "--owner", "devin"]);
    ok(d.path(), &["backlog", "add", "someday idea"]);
    d
}

/// The project name `myco init` recorded (repo dir basename).
fn name_of(d: &Path) -> String {
    d.file_name().unwrap().to_string_lossy().into_owned()
}

#[test]
fn schema_document_snapshot() {
    let out = run(Path::new("/"), &["schema"], None);
    assert_eq!(code(&out), 0);
    let v = json(&out);
    assert_eq!(v["version"], 1);
    assert!(v["types"]["Item"].is_object());
    insta::assert_snapshot!(serde_json::to_string_pretty(&v).unwrap());
}

#[test]
fn status_views() {
    let d = seeded();
    // human
    let out = ok(d.path(), &["status"]);
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.contains("tasks 2o/0c/0ip/0b"), "{s}");
    assert!(s.contains("backlog 1"), "{s}");
    assert!(s.contains("issues 1o"), "{s}");
    // json
    let v = json(&ok(d.path(), &["status", "--json"]));
    assert_eq!(v["counts"]["tasks"]["open"], 2);
    assert_eq!(v["counts"]["backlog"], 1);
    assert_eq!(v["counts"]["issues"]["open"], 1);
    // prompt
    let out = ok(d.path(), &["status", "--prompt"]);
    let s = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<&str> = s.lines().collect();
    assert!(lines.len() <= 60, "{}", lines.len());
    assert!(lines[0].starts_with("repo:"), "{s}");
    assert!(s.contains("open:"), "{s}");
    assert!(s.contains("T-002 high hot task"), "{s}");
    assert!(!s.contains("someday idea"), "deferred stays out: {s}");
    // --prompt --json is still json — {"prompt": "<block>"}
    let v = json(&ok(d.path(), &["status", "--prompt", "--json"]));
    let p = v["prompt"].as_str().unwrap();
    assert!(p.starts_with("repo:"), "{p}");
    assert!(p.contains("T-002 high hot task"), "{p}");
    assert!(p.lines().count() <= 60, "{}", p.lines().count());
}

#[test]
fn prompt_block_stays_under_sixty_lines() {
    let d = repo();
    init(d.path());
    // hand-seed a huge TODO.md — 80 open tasks
    let wt = d.path().join("project");
    let name = name_of(d.path());
    let mut todo = format!("# TODO — {name}\n\n## big\n\n");
    for n in 1..=80 {
        todo.push_str(&format!("- [ ] T-{n:03} task number {n}\n"));
    }
    std::fs::write(wt.join("TODO.md"), todo).unwrap();
    std::fs::write(
        wt.join("meta.toml"),
        format!("name = \"{name}\"\n\n[next]\nt = 81\n"),
    )
    .unwrap();
    let out = ok(d.path(), &["status", "--prompt"]);
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.lines().count() <= 60, "{}", s.lines().count());
    assert!(s.contains("… +"), "{s}");
}

#[test]
fn board_views() {
    let d = seeded();
    ok(
        d.path(),
        &["item", "set", "T-002", "--status", "in-progress"],
    );
    ok(
        d.path(),
        &["item", "set", "I-001", "--status", "in-progress"],
    );
    ok(d.path(), &["item", "set", "I-001", "--status", "blocked"]);
    // human grid
    let out = ok(d.path(), &["view", "board"]);
    let s = String::from_utf8_lossy(&out.stdout);
    for col in ["Backlog", "Open", "Claimed", "In Progress", "Blocked"] {
        assert!(s.contains(col), "{col} missing:\n{s}");
    }
    assert!(s.contains("T-002 hot task"), "{s}");
    assert!(s.contains("I-001 a bug"), "{s}");
    // done items have no column
    // md output — padded markdown table
    let out = ok(d.path(), &["view", "board", "--md"]);
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.lines().next().unwrap_or("").starts_with('|'), "{s}");
    assert!(s.contains("Backlog"), "{s}");
    assert!(s.contains("---"), "{s}");
    // json columns map
    let v = json(&ok(d.path(), &["view", "board", "--json"]));
    assert_eq!(v["backlog"].as_array().unwrap().len(), 1);
    assert_eq!(v["open"].as_array().unwrap().len(), 1);
    assert_eq!(v["in-progress"].as_array().unwrap()[0]["id"], "T-002");
    assert_eq!(v["blocked"].as_array().unwrap()[0]["id"], "I-001");
    assert!(v["claimed"].as_array().unwrap().is_empty());
    // interrupted items carry the flag in --json
    ok(d.path(), &["interrupt", "T-002"]);
    let v = json(&ok(d.path(), &["view", "board", "--json"]));
    let open = v["open"].as_array().unwrap();
    assert_eq!(
        open.iter().find(|i| i["id"] == "T-002").unwrap()["interrupted"],
        true
    );
    assert_eq!(
        open.iter().find(|i| i["id"] == "T-001").unwrap()["interrupted"],
        false
    );
    // status --json keeps the id-list shape alongside the flag
    let v = json(&ok(d.path(), &["status", "--json"]));
    assert_eq!(v["interrupted"], serde_json::json!(["T-002"]));
}

#[test]
fn report_aging_and_velocity() {
    let d = seeded();
    // synthetic history: one add (todo add) + one done
    ok(d.path(), &["todo", "done", "T-001"]);
    let v = json(&ok(d.path(), &["report", "--json"]));
    assert!(v["aging"]["<7d"].as_u64().unwrap() >= 1);
    let vel = v["velocity"].as_array().unwrap();
    let new: u64 = vel.iter().map(|w| w["new"].as_u64().unwrap()).sum();
    let done: u64 = vel.iter().map(|w| w["done"].as_u64().unwrap()).sum();
    // 2 todo adds + 1 issue new + 1 backlog add = 4 creations;
    // 1 todo done = 1 completion
    assert_eq!(new, 4, "{vel:?}");
    assert_eq!(done, 1, "{vel:?}");
    // human
    let out = ok(d.path(), &["report"]);
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.contains("aging:"), "{s}");
    assert!(s.contains("velocity:"), "{s}");
    assert!(s.contains("+"), "{s}");
}

#[test]
fn index_add_list_rebuild_remove() {
    let home = tempfile::tempdir().unwrap();
    let d = seeded();
    let canon = d.path().canonicalize().unwrap();
    // add
    let out = run(
        d.path(),
        &["index", "add", &canon.to_string_lossy(), "--json"],
        Some(home.path()),
    );
    assert_eq!(code(&out), 0, "{out:?}");
    let v = json(&out);
    assert_eq!(v["registered"]["name"], name_of(d.path()));
    assert_eq!(v["snapshot"]["items"].as_u64().unwrap(), 4);
    // index files exist under MYCO_HOME
    assert!(home.path().join("index.redb").exists());
    assert!(home.path().join("repos.toml").exists());
    // list — fresh right after add
    let out = run(d.path(), &["index", "list", "--json"], Some(home.path()));
    let v = json(&out);
    assert_eq!(v[0]["stale"], false);
    // a ledger change moves HEAD → snapshot goes stale
    ok(d.path(), &["todo", "add", "another"]);
    let out = run(d.path(), &["index", "list", "--json"], Some(home.path()));
    assert_eq!(json(&out)[0]["stale"], true);
    // rebuild restores freshness
    let out = run(
        d.path(),
        &["index", "rebuild", &canon.to_string_lossy(), "--json"],
        Some(home.path()),
    );
    assert_eq!(code(&out), 0, "{out:?}");
    let v = json(&out);
    assert_eq!(v[0]["status"], "rebuilt");
    assert_eq!(v[0]["snapshot"]["items"].as_u64().unwrap(), 5);
    let out = run(d.path(), &["index", "list", "--json"], Some(home.path()));
    assert_eq!(json(&out)[0]["stale"], false);
    // remove
    let out = run(
        d.path(),
        &["index", "remove", &canon.to_string_lossy()],
        Some(home.path()),
    );
    assert_eq!(code(&out), 0);
    let v = json(&run(
        d.path(),
        &["index", "list", "--json"],
        Some(home.path()),
    ));
    assert!(v.as_array().unwrap().is_empty());
    // unregistered remove errors
    let out = run(
        d.path(),
        &["index", "remove", &canon.to_string_lossy()],
        Some(home.path()),
    );
    assert_eq!(code(&out), 2);
}

#[test]
fn index_rejects_non_repos() {
    let home = tempfile::tempdir().unwrap();
    let plain = tempfile::tempdir().unwrap();
    let out = run(
        plain.path(),
        &["index", "add", &plain.path().to_string_lossy()],
        Some(home.path()),
    );
    assert_eq!(code(&out), 2);
    // rebuild of an unregistered repo errors
    let d = seeded();
    let canon = d.path().canonicalize().unwrap();
    let out = run(
        d.path(),
        &["index", "rebuild", &canon.to_string_lossy()],
        Some(home.path()),
    );
    assert_eq!(code(&out), 2);
    assert!(String::from_utf8_lossy(&out.stderr).contains("not registered"));
}

#[test]
fn org_status_and_board_across_repos() {
    let home = tempfile::tempdir().unwrap();
    let a = seeded();
    let b = seeded();
    for r in [&a, &b] {
        let canon = r.path().canonicalize().unwrap();
        let out = run(
            r.path(),
            &["index", "add", &canon.to_string_lossy()],
            Some(home.path()),
        );
        assert_eq!(code(&out), 0, "{out:?}");
    }
    let (na, nb) = (name_of(a.path()), name_of(b.path()));
    // org-status — the index-sourced, repo-grouped list view
    let out = run(
        a.path(),
        &["view", "org-status", "--json"],
        Some(home.path()),
    );
    assert_eq!(code(&out), 0, "{out:?}");
    let v = json(&out);
    assert_eq!(v["view"], "org-status");
    assert_eq!(v["spec"]["source"], "index");
    assert_eq!(v["spec"]["group_by"], "repo");
    let groups = v["groups"].as_array().unwrap();
    // one bucket per repo, named by the repo tag
    let labels: std::collections::BTreeSet<&str> =
        groups.iter().filter_map(|g| g["label"].as_str()).collect();
    let mut want = vec![na.clone(), nb.clone()];
    want.sort();
    assert_eq!(labels.iter().copied().collect::<Vec<_>>(), want);
    // each repo seeded 4 live items (2 tasks + 1 issue + 1 backlog)
    for g in groups {
        assert_eq!(g["items"].as_array().unwrap().len(), 4, "{g}");
        // every item carries its repo tag
        assert_eq!(g["items"][0]["repo"], g["label"]);
    }
    // org board — repo-prefixed cards (2 tasks + 1 issue open each)
    let v = json(&run(
        a.path(),
        &["view", "org-board", "--json"],
        Some(home.path()),
    ));
    assert_eq!(v["open"].as_array().unwrap().len(), 6);
    let repos: std::collections::BTreeSet<&str> = v["open"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["repo"].as_str().unwrap())
        .collect();
    assert_eq!(repos.iter().copied().collect::<Vec<_>>(), want);
    // snapshots carry no handoffs — no interrupted flag on items
    assert!(v["open"][0].get("interrupted").is_none());
    // human org board mentions repo prefixes
    let out = run(a.path(), &["view", "org-board"], Some(home.path()));
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.contains(&format!("{na}:T-")), "{s}");
    assert!(s.contains(&format!("{nb}:T-")), "{s}");
    // index reads auto-rebuild stale snapshots — a fresh commit
    // shows up in the next org view
    ok(a.path(), &["todo", "add", "fresh work"]);
    let v = json(&run(
        a.path(),
        &["view", "org-status", "--json"],
        Some(home.path()),
    ));
    let total: usize = v["groups"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| g["items"].as_array().unwrap().len())
        .sum();
    assert_eq!(total, 9, "{v}");
}

#[test]
fn index_add_accepts_worktree_and_rebuild_all() {
    let home = tempfile::tempdir().unwrap();
    let d = seeded();
    let wt = d.path().join("project").canonicalize().unwrap();
    // registering the worktree path resolves to the repo root
    let out = run(
        d.path(),
        &["index", "add", &wt.to_string_lossy(), "--json"],
        Some(home.path()),
    );
    assert_eq!(code(&out), 0, "{out:?}");
    assert_eq!(
        json(&out)["registered"]["path"].as_str().unwrap(),
        d.path().canonicalize().unwrap().to_string_lossy()
    );
    // `index rebuild` with no path rebuilds every registered repo
    ok(d.path(), &["todo", "add", "one more"]);
    let out = run(d.path(), &["index", "rebuild", "--json"], Some(home.path()));
    let v = json(&out);
    assert_eq!(v.as_array().unwrap().len(), 1);
    assert_eq!(v[0]["status"], "rebuilt");
    assert_eq!(v[0]["snapshot"]["items"].as_u64().unwrap(), 5);
    // human list line
    let out = run(d.path(), &["index", "list"], Some(home.path()));
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.contains("fresh"), "{s}");
}

#[test]
fn org_empty_index() {
    let home = tempfile::tempdir().unwrap();
    let d = seeded();
    let out = run(
        d.path(),
        &["view", "org-status", "--json"],
        Some(home.path()),
    );
    assert_eq!(code(&out), 0);
    let v = json(&out);
    assert_eq!(v["groups"].as_array().unwrap().len(), 0);
    assert_eq!(v["interrupted"], serde_json::json!([]));
    // human form reports the empty view, exit 0
    let out = run(d.path(), &["view", "org-status"], Some(home.path()));
    assert_eq!(code(&out), 0);
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("empty view"),
        "{out:?}"
    );
}

#[test]
fn org_view_error_warns_and_identifies_the_repo() {
    let home = tempfile::tempdir().unwrap();
    let d = seeded();
    let canon = d.path().canonicalize().unwrap();
    let out = run(
        d.path(),
        &["index", "add", &canon.to_string_lossy()],
        Some(home.path()),
    );
    assert_eq!(code(&out), 0, "{out:?}");
    // no snapshot + a vanished worktree → the repo contributes
    // nothing, and the read must still say *which* repo failed —
    // a stderr warning naming it, not a silent skip
    std::fs::remove_file(home.path().join("index.redb")).unwrap();
    std::fs::remove_dir_all(d.path().join("project")).unwrap();
    let out = run(
        d.path(),
        &["view", "org-status", "--json"],
        Some(home.path()),
    );
    assert_eq!(code(&out), 0, "{out:?}");
    let v = json(&out);
    assert!(v["groups"].as_array().unwrap().is_empty(), "{v}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("warning"), "{stderr}");
    assert!(stderr.contains(&name_of(d.path())), "{stderr}");
    assert!(stderr.contains("worktree"), "{stderr}");
}

#[test]
fn default_view_renders_instead_of_listing() {
    let d = seeded();
    // `default_view` in the repo layer names the landing view —
    // bare `myco view` renders it rather than listing views
    std::fs::write(d.path().join("myco.toml"), "default_view = \"open\"\n").unwrap();
    let cfg = d.path().join("myco.toml");
    let envs = [
        ("MYCO_CONFIG", cfg.to_str().unwrap()),
        ("XDG_CONFIG_HOME", "/nonexistent-xdg"),
        ("HOME", "/nonexistent-home"),
    ];
    let out = run_env(d.path(), &["view"], None, &envs);
    assert_eq!(code(&out), 0, "{out:?}");
    let s = String::from_utf8_lossy(&out.stdout);
    // the `open` preset renders items, not the `name origin spec`
    // list columns
    assert!(s.contains("T-001"), "{s}");
    assert!(!s.contains("builtin"), "{s}");
    // `--list` still enumerates
    let out = run_env(d.path(), &["view", "--list"], None, &envs);
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.contains("builtin"), "{s}");
}

#[test]
fn view_repos_format_rows_counts_and_error() {
    let home = tempfile::tempdir().unwrap();
    let base = tempfile::tempdir().unwrap();
    // two repos under one parent dir — the "org" column names it
    let a = base.path().join("acme").join("alpha");
    let b = base.path().join("acme").join("beta");
    seeded_at(&a);
    seeded_at(&b);
    for r in [&a, &b] {
        let canon = r.canonicalize().unwrap();
        let out = run(
            r,
            &["index", "add", &canon.to_string_lossy()],
            Some(home.path()),
        );
        assert_eq!(code(&out), 0, "{out:?}");
    }
    // `[views.repos]` in the repo layer names the repos format
    std::fs::write(
        a.join("myco.toml"),
        "[views.repos]\nsource = \"index\"\nformat = \"repos\"\n",
    )
    .unwrap();
    let sealed = [
        ("XDG_CONFIG_HOME", "/nonexistent-xdg"),
        ("HOME", "/nonexistent-home"),
    ];
    // human table — one row per registered repo
    let out = run_env(&a, &["view", "repos"], Some(home.path()), &sealed);
    assert_eq!(code(&out), 0, "{out:?}");
    let s = String::from_utf8_lossy(&out.stdout);
    for want in [
        "repo", "org", "items", "open", "claimed", "err", "alpha", "beta", "acme",
    ] {
        assert!(s.contains(want), "{want} missing:\n{s}");
    }
    // --md falls out of the tabled renderer
    let out = run_env(&a, &["view", "repos", "--md"], Some(home.path()), &sealed);
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.lines().next().unwrap_or("").starts_with('|'), "{s}");
    // json — counts from each snapshot's live ledger (4 live:
    // 2 tasks + 1 owned issue + 1 deferred backlog item)
    let v = json(&run_env(
        &a,
        &["view", "repos", "--json"],
        Some(home.path()),
        &sealed,
    ));
    assert_eq!(v["view"], "repos");
    assert_eq!(v["spec"]["format"], "repos");
    let rows = v["repos"].as_array().unwrap();
    assert_eq!(rows.len(), 2, "{v}");
    for r in rows {
        assert_eq!(r["org"], "acme", "{r}");
        assert_eq!(r["counts"]["total"], 4, "{r}");
        assert_eq!(r["counts"]["open"], 3, "{r}");
        assert_eq!(r["counts"]["claimed"], 1, "{r}");
        assert!(r["error"].is_null(), "{r}");
    }
    // outside a repo — an ad-hoc spec still reaches the index
    let plain = tempfile::tempdir().unwrap();
    let out = run_env(
        plain.path(),
        &["view", "--format", "repos", "--source", "index", "--json"],
        Some(home.path()),
        &sealed,
    );
    assert_eq!(code(&out), 0, "{out:?}");
    assert_eq!(json(&out)["repos"].as_array().unwrap().len(), 2);
    // and a config layer outside any repo can name `view repos`
    // (a machine `~/myco.toml` — here pinned via $MYCO_CONFIG —
    // plus `default_view` making it the bare `myco view` landing)
    let cfg = tempfile::tempdir().unwrap();
    let cfgfile = cfg.path().join("myco.toml");
    std::fs::write(
        &cfgfile,
        "default_view = \"repos\"\n\n[views.repos]\nsource = \"index\"\nformat = \"repos\"\n",
    )
    .unwrap();
    let envs = [
        ("MYCO_CONFIG", cfgfile.to_str().unwrap()),
        ("XDG_CONFIG_HOME", "/nonexistent-xdg"),
        ("HOME", "/nonexistent-home"),
    ];
    let out = run_env(plain.path(), &["view", "--json"], Some(home.path()), &envs);
    assert_eq!(code(&out), 0, "{out:?}");
    let v = json(&out);
    assert_eq!(v["view"], "repos");
    assert_eq!(v["repos"].as_array().unwrap().len(), 2);
    // a vanished worktree → the row keeps its identity, names the
    // failure, and reports `—`/null counts (the stale snapshot
    // doesn't masquerade as live)
    std::fs::remove_dir_all(b.join("project")).unwrap();
    let v = json(&run_env(
        &a,
        &["view", "repos", "--json"],
        Some(home.path()),
        &sealed,
    ));
    let rows = v["repos"].as_array().unwrap();
    let beta = rows.iter().find(|r| r["name"] == "beta").unwrap();
    assert!(beta["counts"].is_null(), "{beta}");
    assert!(
        beta["error"].as_str().unwrap().contains("worktree"),
        "{beta}"
    );
    let alpha = rows.iter().find(|r| r["name"] == "alpha").unwrap();
    assert_eq!(alpha["counts"]["total"], 4, "{alpha}");
    // human — the error repo's dash cells + terse err text
    let out = run_env(&a, &["view", "repos"], Some(home.path()), &sealed);
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.contains('—'), "{s}");
    assert!(s.contains("worktree"), "{s}");
    // a repo-sourced repos view is a single row for this repo
    let v = json(&run_env(
        &a,
        &["view", "--format", "repos", "--json"],
        Some(home.path()),
        &sealed,
    ));
    let rows = v["repos"].as_array().unwrap();
    assert_eq!(rows.len(), 1, "{v}");
    assert_eq!(rows[0]["name"], "alpha");
    assert_eq!(rows[0]["counts"]["total"], 4);
}
