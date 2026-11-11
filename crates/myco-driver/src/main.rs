//! tamias-driver-myco — the pavo view driver for a repo's
//! `project/` ledger.
//!
//! Verbs (the tamias driver contract):
//!
//! - `manifest` — print the [`DriverManifest`] JSON (discovery
//!   probe; never touches project state).
//! - `capabilities` — `{"capabilities": ["pavo"], "verbs": […]}`.
//! - `ops` — the engine ops manifest: every `myco` CLI verb as a
//!   `cli.<verb>` wire query, derived from the engine's own
//!   [`myco::registry::VERBS`] table.
//! - `run <argv>` — engine passthrough: `tamias-driver-myco run
//!   board` ≡ `myco board`. Also the child side of `cli.*` wire
//!   queries (`ops::run_verb` re-execs this same verb so engine
//!   output never touches the serve stream).
//! - `cli <myco argv>` — the tamias `bins[]` shim verb (T-006):
//!   `tamias bin exec` calls it as `cli --dir <pkg> --name myco
//!   <argv>` for the `shim: cli` bin declared in
//!   `libraries/myco/tamias.yaml`. myco's engine IS the CLI, so
//!   this is a passthrough — strip the two standard driver flags
//!   and hand the rest to `myco::run_from` with inherited stdio.
//!   That keeps full argv parity the retired `myco` binary had:
//!   `--help`/`--version`, every bounded verb, and the unbounded
//!   surfaces (`myco serve` JSON-RPC, `myco ui` TUI, `myco batch`)
//!   all stream end-to-end.
//! - `serve` — ndjson JSON-RPC on stdio: `{"op":"manifest"}`,
//!   `{"op":"views"}`, `{"op":"view","view":…,"params":{…}}`,
//!   `{"op":"query","query":…,"params":{…}}` →
//!   `{"ok":true,"result":…}` / `{"ok":false,"error":…}`.
//!   `query:"ops"` answers the ops manifest; `query:"cli.<verb>"`
//!   runs the engine op.
//!
//! Optional surface: feature `mcp` embeds pavo-driver-kit's
//! surface layer — `mcp` serves the ops manifest as MCP tools on
//! stdio, equivalent to the standalone `tamias-driver-mcp` minus
//! the PATH/discovery hop. There is no `cli` feature: `cli` is the
//! shim verb above, not a generated surface.
//!
//! Which repo: each `view`/`query` resolves its own target —
//! `params.dir`/`params.root` → `$MYCO_DIR` → cwd discovery
//! (see `data.rs` for the exact order). Engine ops are the
//! exception by design: `cli.<verb>` re-execs `run` in the
//! request's repo and lets `myco::dispatch` own the outcome —
//! writes keep the engine's own semantics (per-verb commit,
//! `Actor:` trailer). `manifest`/`views` are project-free so
//! pavo can enumerate drivers anywhere.

mod data;
mod ops;
#[cfg(feature = "mcp")]
mod surface;
mod views;

use pavo_driver_kit as kit;
use pavo_protocol::wire::{self, Request, Response, ViewSummary};

fn capabilities_json() -> serde_json::Value {
    let m = views::manifest();
    serde_json::json!({
        "capabilities": m.capabilities,
        "verbs": m.verbs,
        "views": m.views.len(),
    })
}

/// Dispatch one parsed request to its response.
fn dispatch(req: &Request) -> Response {
    match req {
        Request::Manifest => Response::ok_typed(&views::manifest()),
        Request::Views => {
            let summaries: Vec<ViewSummary> =
                views::specs().iter().map(ViewSummary::from).collect();
            Response::ok_typed(&summaries)
        }
        Request::View { view, params } => match views::view_result(view, params) {
            Ok(result) => Response::ok_typed(&result),
            Err(e) => Response::err(e),
        },
        Request::Query { query, params } => {
            // `ops` and `cli.<verb>` are engine ops; everything
            // else is a view query — unknown names fail the op,
            // degraded snapshots travel as `{"error": …}` data
            // (the marker renderers understand).
            if query == "ops" {
                Response::ok_typed(&wire::QueryResult {
                    query: query.clone(),
                    data: ops::manifest(),
                })
            } else if let Some(v) = ops::op_of(query) {
                ops::run_verb(v, params)
            } else if views::known_query(query) {
                let snap = data::load(params);
                let payload = views::query_data(query, params, &snap);
                Response::ok_typed(&wire::QueryResult {
                    query: query.clone(),
                    data: payload,
                })
            } else {
                Response::err(format!("unknown query: {query}"))
            }
        }
        Request::Render { kind, .. } => Response::err(format!(
            "tamias-driver-myco claims no renderers (kind '{kind}')"
        )),
        // The protocol grew `submit`/`jobs`/`notifications` ops after
        // this driver was written; the view side is read-only with
        // no feeds, so they answer `ok:false` like any unsupported
        // op. Mutations go through `cli.<verb>`, never `submit`.
        Request::Submit { .. } | Request::Jobs | Request::Notifications => {
            Response::err("tamias-driver-myco implements no submit/jobs/notifications ops")
        }
    }
}

fn serve() -> i32 {
    // The kit owns the ndjson loop — stdin EOF and a dead client
    // both end it quietly, and either way the driver exits 0.
    let _ = kit::serve_stdio(dispatch);
    0
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match args.first().map(String::as_str) {
        Some("manifest") => {
            println!("{}", kit::manifest_json(&views::manifest()));
            0
        }
        Some("capabilities") => {
            println!(
                "{}",
                serde_json::to_string_pretty(&capabilities_json()).unwrap_or_default()
            );
            0
        }
        Some("ops") => {
            println!("{}", ops::manifest());
            0
        }
        // Engine passthrough — `tamias-driver-myco run status` is
        // `myco status`. Also the child side of `cli.*` wire
        // queries (ops::run_verb re-execs this same verb).
        Some("run") => {
            let argv = std::iter::once("myco".to_string()).chain(args.iter().skip(1).cloned());
            std::process::exit(myco::run_from(argv));
        }
        // The tamias `bins[]` shim verb (T-006): `tamias bin exec`
        // runs `<driver> cli --dir <pkg> --name myco <myco argv>`
        // for the declared `shim: cli` bin in
        // `libraries/myco/tamias.yaml`. myco's engine IS the CLI,
        // so this is a passthrough — strip the two standard driver
        // flags and hand the rest to `myco::run_from` with
        // inherited stdio. Full engine parity: `--help`/`--version`,
        // every bounded verb, and the unbounded surfaces (`serve`
        // JSON-RPC, `ui` TUI, `batch`) behave exactly like the
        // retired `myco` binary — the ledger verbs other repos and
        // agents call constantly.
        Some("cli") => {
            let argv = engine_argv(&args[1..]);
            std::process::exit(myco::run_from(argv));
        }
        // Optional embedded surface (feature-gated): the
        // pavo-driver-kit surface layer renders this driver's own
        // ops manifest as MCP tools over stdio — the ops→tools
        // rendering of the same `cli.*` queries `serve` answers.
        // (The engine's own surfaces — `myco serve`/`ui`/`batch` —
        // are reached through `run`/`cli`, never this verb.)
        #[cfg(feature = "mcp")]
        Some("mcp") => {
            let m = kit::surface::OpsManifest::from_value(&ops::manifest()).unwrap_or_default();
            match kit::surface::mcp::serve_stdio(&m, &mut surface::Engine) {
                Ok(()) => 0,
                Err(e) => {
                    eprintln!("tamias-driver-myco mcp: {e}");
                    1
                }
            }
        }
        Some("serve") | None => serve(),
        _ => {
            eprintln!(
                "usage: tamias-driver-myco manifest|capabilities|ops|run <argv>|cli <myco argv>|serve\n       (feature `mcp` adds `mcp`)"
            );
            1
        }
    };
    std::process::exit(code);
}

/// `cli` verb argv (after the `cli` token) → engine argv (argv[0]
/// included). `tamias bin exec` prepends the standard driver flags
/// — exactly `--dir <entry> --name <pkg>` — which the engine
/// doesn't know; consume that one leading pair (`--flag v` and
/// `--flag=v` forms, either order) and pass everything else through
/// untouched.
///
/// Deliberately NOT a "leading run" strip: myco's engine has its
/// own global `--dir`/`--root` flags, so `myco --dir <wt> status`
/// typed through the shim arrives as `cli --dir <entry> --name myco
/// --dir <wt> status` — the second `--dir` is the user's and must
/// reach the engine's parse. Each driver flag is consumed at most
/// once; the first repeat ends the scan.
fn engine_argv(args: &[String]) -> Vec<String> {
    let mut out = vec!["myco".to_string()];
    let mut seen_dir = false;
    let mut seen_name = false;
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        let (flag, takes_value) = if a == "--dir" || a.starts_with("--dir=") {
            ("dir", !a.contains('='))
        } else if a == "--name" || a.starts_with("--name=") {
            ("name", !a.contains('='))
        } else {
            break;
        };
        let seen = match flag {
            "dir" => seen_dir,
            _ => seen_name,
        };
        if seen {
            break;
        }
        match flag {
            "dir" => seen_dir = true,
            _ => seen_name = true,
        }
        // `--flag v` consumes the value too (a trailing flag with
        // no value just ends the scan); `--flag=v` is self-contained.
        i += if takes_value { 2 } else { 1 };
        i = i.min(args.len());
    }
    out.extend(args[i..].iter().cloned());
    out
}
