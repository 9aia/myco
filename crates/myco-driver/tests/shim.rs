//! `cli` verb — the tamias `bins[]` shim passthrough (T-006).
//!
//! `tamias bin exec` invokes `tamias-driver-myco cli --dir <pkg>
//! --name myco <myco argv>`; the verb strips the leading driver
//! flags and hands the rest to `myco::run_from` with inherited
//! stdio — the same engine path `run` and the `cli.*` wire queries
//! take. These tests pin the strip rules: `--dir`/`--name` in
//! either `--flag v` or `--flag=v` form are consumed *only* as the
//! leading run, so myco's own global `--dir`/`--root` flags (which
//! share the names) still reach the engine's clap parse.

#![allow(clippy::unwrap_used)]

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Output, Stdio};

use serde_json::{json, Value};

const BIN: &str = env!("CARGO_BIN_EXE_tamias-driver-myco");

/// `tamias-driver-myco cli <args…>` — the shim entry point.
fn cli(dir: &Path, args: &[&str]) -> Output {
    Command::new(BIN)
        .arg("cli")
        .args(args)
        .current_dir(dir)
        .env("MYCO_CONFIG", "/nonexistent/myco.toml")
        .env_remove("MYCO_NO_COMMIT")
        .output()
        .unwrap()
}

fn git(dir: &Path, args: &[&str]) {
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

#[test]
fn cli_strips_driver_flags_and_runs_engine() {
    let d = repo();
    // `myco init` through the shim — both `--flag v` and `--flag=v`
    // driver-flag forms, in either order, are consumed before argv.
    let out = cli(
        d.path(),
        &["--dir", "/payload/dir", "--name=myco", "init", "--json"],
    );
    assert!(
        out.status.success(),
        "init via cli failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(d.path().join("project").is_dir());
}

#[test]
fn cli_engine_globals_still_parse() {
    let d = repo();
    let init = cli(d.path(), &["--name", "myco", "init", "--json"]);
    assert!(init.status.success());
    let wt = d.path().join("project");

    // The engine's own `--dir` global shares the driver flag's name
    // — `myco --dir <wt> status` through the shim arrives as
    // `cli --dir <entry> --name myco --dir <wt> status`. Only the
    // driver's pair may be stripped; run from a bare dir so the
    // engine's `--dir` is the only way `status` resolves a project.
    let bare = tempfile::tempdir().unwrap();
    let out = cli(
        bare.path(),
        &[
            "--dir",
            "/entry",
            "--name",
            "myco",
            "--dir",
            wt.to_str().unwrap(),
            "status",
            "--json",
        ],
    );
    assert!(
        out.status.success(),
        "status via cli failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(v["repo"].is_string(), "unexpected status json: {v}");
}

#[test]
fn cli_version_and_help() {
    let d = tempfile::tempdir().unwrap();
    let out = cli(d.path(), &["--name", "myco", "--version"]);
    assert!(out.status.success());
    assert!(!out.stdout.is_empty());
    let out = cli(d.path(), &["--name=myco", "--help"]);
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("myco"));
}

/// A spawned `cli … serve` — the unbounded surface streaming
/// through the shim path with inherited stdio.
struct Serve {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl Serve {
    fn spawn(dir: &Path, args: &[&str]) -> Serve {
        let mut child = Command::new(BIN)
            .arg("cli")
            .args(args)
            .current_dir(dir)
            .env("MYCO_CONFIG", "/nonexistent/myco.toml")
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

    fn rpc(&mut self, req: &Value) -> Value {
        writeln!(self.stdin, "{}", serde_json::to_string(req).unwrap()).unwrap();
        self.stdin.flush().unwrap();
        let mut buf = String::new();
        let n = self.stdout.read_line(&mut buf).unwrap();
        assert!(n > 0, "serve closed stdout");
        serde_json::from_str(&buf).unwrap()
    }

    fn finish(mut self) -> i32 {
        drop(self.stdin);
        self.child.wait().unwrap().code().unwrap_or(-1)
    }
}

#[test]
fn cli_serve_streams_ndjson() {
    let d = repo();
    let init = cli(d.path(), &["--name", "myco", "init", "--json"]);
    assert!(init.status.success());

    // `tamias bin exec` always passes `--dir <pkg>` first — prove
    // the shim strips it and `serve` still owns stdio.
    let mut s = Serve::spawn(d.path(), &["--dir", "/payload", "--name", "myco", "serve"]);
    let v = s.rpc(&json!({"jsonrpc": "2.0", "id": 1, "method": "initialize"}));
    assert_eq!(v["id"], 1);
    assert_eq!(v["result"]["protocol"], 1);
    let v = s.rpc(&json!({"id": 2, "method": "status", "params": {"prompt": true}}));
    assert!(v["result"]["prompt"].as_str().unwrap().contains("repo:"));
    assert_eq!(s.finish(), 0);
}
