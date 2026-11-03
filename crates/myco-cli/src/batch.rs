//! `myco batch` — the one-shot `serve` (P-004): NDJSON verb
//! requests in (stdin or `--file`), one JSON-RPC 2.0 response
//! line out per request, each dispatched through the same
//! [`serve::Server`] surface `myco serve` exposes — the whole
//! verb set without holding a pipe open.
//!
//! Per-line errors are reported in-band and never abort the
//! run; mutations keep serve's per-request commit semantics
//! (`--no-commit`/`$MYCO_NO_COMMIT` opt out). stdin IS the wire
//! here too — requests that would read it (`note "-"`) are
//! rejected exactly as serve rejects them.

use std::io::{BufRead, Write};
use std::path::PathBuf;

use crate::serve::{JsonRpc2, Server, WireProtocol};

/// The `batch` verb — exits nonzero only when the runner itself
/// fails (unreadable `--file`, a write/read error), never on a
/// line's own error response.
pub fn h(ctx: &crate::registry::Ctx, a: &crate::registry::Args) -> Result<i32, String> {
    let file = a.opt("file").map(PathBuf::from);
    let server = Server::new(ctx.dir.clone(), ctx.root.clone(), ctx.no_commit);
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    match file {
        Some(path) => {
            let f = std::fs::File::open(&path)
                .map_err(|e| format!("--file {}: {e}", path.display()))?;
            run_reader(&server, &JsonRpc2, std::io::BufReader::new(f), &mut out)
        }
        None => {
            let stdin = std::io::stdin();
            run_reader(&server, &JsonRpc2, stdin.lock(), &mut out)
        }
    }
}

/// Read → dispatch → write, one line at a time until EOF.
fn run_reader<R: BufRead, W: WireProtocol, O: Write>(
    server: &Server,
    wire: &W,
    reader: R,
    out: &mut O,
) -> Result<i32, String> {
    for line in reader.lines() {
        let line = line.map_err(|e| format!("read request: {e}"))?;
        if let Some(response) = server.handle_line(wire, &line) {
            writeln!(out, "{response}").map_err(|e| format!("write response: {e}"))?;
        }
    }
    out.flush().map_err(|e| format!("write response: {e}"))?;
    Ok(0)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    /// A scaffolded worktree under an explicit root — `dir` =
    /// `<d>/project`, `root` = `d` (no git needed while
    /// `no_commit` holds).
    fn fixture(no_commit: bool) -> (tempfile::TempDir, Server) {
        let d = tempfile::tempdir().unwrap();
        let wt = d.path().join("project");
        std::fs::create_dir(&wt).unwrap();
        let store =
            myco_store_md::MdStore::new(camino::Utf8PathBuf::from_path_buf(wt.clone()).unwrap());
        crate::ops::scaffold(&store, &wt, "p").unwrap();
        let s = Server::new(Some(wt), Some(d.path().to_path_buf()), no_commit);
        (d, s)
    }

    fn git(dir: &std::path::Path, args: &[&str]) -> String {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap().trim().to_string()
    }

    /// NDJSON in → the parsed response lines out.
    fn responses(s: &Server, input: &str) -> Vec<Value> {
        let mut out = Vec::new();
        assert_eq!(
            run_reader(s, &JsonRpc2, input.as_bytes(), &mut out).unwrap(),
            0
        );
        String::from_utf8(out)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    #[test]
    fn mixed_success_error_and_malformed_lines() {
        let (_d, s) = fixture(true);
        let input = concat!(
            "{\"id\":1,\"method\":\"manifest\"}\n",
            "\n",        // blank — skipped
            "   \n",     // whitespace — skipped
            "garbage\n", // parse error
            "{\"id\":\"a\",\"method\":\"status\"}\n",
            "{\"id\":3,\"method\":\"nope\"}\n",
            "{\"method\":\"status\"}\n", // missing id
            "{\"id\":4,\"method\":\"todo.add\",\"params\":{}}\n"  // bad params
        );
        let lines = responses(&s, input);
        assert_eq!(lines.len(), 6, "{lines:?}");
        // ids correlate per line
        assert_eq!(lines[0]["id"], 1);
        assert_eq!(lines[0]["result"]["name"], "myco");
        // malformed → error response with null id, run continues
        assert_eq!(lines[1]["id"], Value::Null);
        assert_eq!(lines[1]["error"]["code"], -32700);
        assert_eq!(lines[2]["id"], "a");
        assert!(lines[2]["result"]["repo"].is_string());
        assert_eq!(lines[3]["id"], 3);
        assert_eq!(lines[3]["error"]["code"], -32601);
        assert_eq!(lines[4]["id"], Value::Null);
        assert_eq!(lines[4]["error"]["code"], -32600);
        assert_eq!(lines[5]["id"], 4);
        assert_eq!(lines[5]["error"]["code"], -32602);
    }

    #[test]
    fn mutations_roundtrip_through_the_store() {
        let (_d, s) = fixture(true);
        let lines = responses(
            &s,
            concat!(
                "{\"id\":1,\"method\":\"todo.add\",\"params\":{\"title\":\"x\"}}\n",
                "{\"id\":2,\"method\":\"todo.list\"}\n",
                "{\"id\":3,\"method\":\"view\",\"params\":{\"name\":\"open\"}}\n",
                "{\"id\":4,\"method\":\"handoff\",\"params\":{\"id\":\"T-001\",\"note\":\"-\"}}\n"
            ),
        );
        assert_eq!(lines[0]["result"]["item"]["id"], "T-001");
        assert_eq!(lines[0]["result"]["committed"], false); // no_commit
                                                            // the next request re-opens the store — the write is there
        assert_eq!(lines[1]["result"].as_array().unwrap().len(), 1);
        assert_eq!(lines[2]["result"]["view"], "open");
        // note "-" can't read stdin — stdin is the wire here too
        assert_eq!(lines[3]["error"]["code"], -32602);
    }

    #[test]
    fn mutations_commit_per_request() {
        // commits on → each mutating request is its own commit,
        // like serve (git_ident supplies a fallback identity)
        let (d, s) = fixture(false);
        let wt = d.path().join("project");
        git(&wt, &["init", "-q"]);
        temp_env::with_var("MYCO_NO_COMMIT", None::<&str>, || {
            let lines = responses(
                &s,
                concat!(
                    "{\"id\":1,\"method\":\"todo.add\",\"params\":{\"title\":\"one\"}}\n",
                    "{\"id\":2,\"method\":\"todo.add\",\"params\":{\"title\":\"two\"}}\n"
                ),
            );
            assert_eq!(lines[0]["result"]["committed"], true);
            assert_eq!(lines[1]["result"]["committed"], true);
        });
        let log = git(&wt, &["log", "--format=%s", "-2"]);
        assert_eq!(log, "chore: add T-002\nchore: add T-001");
        // worktree clean — every write committed
        assert_eq!(git(&wt, &["status", "--porcelain"]), "");
    }

    #[test]
    fn missing_file_fails_the_runner_not_a_line() {
        let mut ctx = crate::registry::Ctx {
            no_commit: true,
            ..Default::default()
        };
        let a = crate::registry::Args::from_argv(
            crate::registry::find("batch").unwrap(),
            &["--file".into(), "/definitely/not/here.ndjson".into()],
            &mut ctx,
            false,
        )
        .unwrap();
        let err = h(&ctx, &a).unwrap_err();
        assert!(err.contains("--file"), "{err}");
        // a perfectly valid batch run still exits 0 with per-line
        // errors inside — covered by mixed_success_* above
    }

    #[test]
    fn serve_method_error_shape_is_jsonrpc() {
        let (_d, s) = fixture(true);
        let line = s
            .handle_line(&JsonRpc2, &json!({"id": 9, "method": "view"}).to_string())
            .unwrap();
        let v: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["jsonrpc"], "2.0");
        assert_eq!(v["id"], 9);
        assert!(!line.contains('\n'), "one response = one line");
    }
}
