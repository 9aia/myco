//! myco — project management, grown not built. A mycorrhizal
//! network for your agents: tasks, signals, handoffs.
//!
//! Agent-first PM: markdown in `project/` is canonical and
//! human-editable; every deterministic write goes through this CLI
//! (auto-commit per verb, `--json` everywhere). The skill teaches
//! agents the verbs; `myco manifest`/`myco schema` are the
//! self-describing surface.
//!
//! Phase 6 wires continuity — `handoff`/`resume` over append-only
//! `handoffs/<ID>.md` files (one per item, any kind); sepia hooks
//! call them at session boundaries. Phase 7 lands the Jira-parity
//! slice — `epic:`/`ms:` item metadata + grouping views,
//! `absorb` importers (comment sweep, Jira CSV, GitHub dumps).
//! P-007 makes the verb surface data: one [`registry`] table
//! drives the clap tree, `manifest`, and serve/batch dispatch;
//! `[verbs]` in `myco.toml` declares repo/user verbs.

use std::path::PathBuf;

mod absorb;
mod adr;
mod agentmd;
mod backlog;
mod batch;
mod board;
mod cfgverb;
mod check;
mod commit;
mod confirm;
mod continuity;
mod digest;
mod discover;
mod done;
mod indexcmd;
mod init;
mod issue;
mod itemcmd;
mod items;
mod links;
mod migrate;
mod movecmd;
mod next;
mod ops;
mod org;
mod plan;
mod proposal;
mod redo;
mod registry;
mod report;
mod schema;
mod serve;
mod status;
mod templates;
mod todo;
mod undo;
mod view;

fn manifest() -> serde_json::Value {
    serde_json::json!({
        "name": "myco",
        "version": env!("CARGO_PKG_VERSION"),
        "protocol": 1,
        "description": env!("CARGO_PKG_DESCRIPTION"),
        "repository": env!("CARGO_PKG_REPOSITORY"),
        "capabilities": {
            "store": "markdown",
            "index": "redb",
            "protocol": 1,
            "serve": {
                "protocol": 1,
                "transport": "stdio",
                "wire": "jsonrpc-2.0",
            },
        },
        "verbs": registry::manifest_verbs(),
        "implemented": registry::manifest_implemented(),
        "notes": "markdown in project/ is canonical; every verb is one \
                  atomic commit on the `project` branch carrying an \
                  `Actor:` trailer (the verb's actor, default `cli`); \
                  the redb index under $MYCO_HOME|~/.local/share/myco is \
                  a disposable derived cache",
    })
}

/// The `manifest` verb.
fn h_manifest(_ctx: &registry::Ctx, _a: &registry::Args) -> Result<i32, String> {
    ops::print_json(&manifest());
    Ok(0)
}

/// The `gen-cli-docs` verb — the docs pipeline reads this.
fn h_gen_docs(_ctx: &registry::Ctx, _a: &registry::Args) -> Result<i32, String> {
    println!(
        "{}",
        clap_markdown::help_markdown_command(&registry::command())
    );
    Ok(0)
}

/// The `ui` verb (feature `tui`). The index universe for
/// `source = "index"` views is injected as a loader — the TUI
/// re-runs it on every reload, so `load_fresh`'s staleness
/// logic governs `:w` too.
#[cfg(feature = "tui")]
fn h_ui(ctx: &registry::Ctx, a: &registry::Args) -> Result<i32, String> {
    let view = a.opt("view").map(str::to_string).or_else(|| {
        // config `default_view` is the landing view — cwd falls
        // back for index-sourced views outside a repo (`~/myco.toml`)
        let root = ctx
            .root
            .clone()
            .or_else(|| ctx.dir.clone())
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        myco_ops::config::default_view_for(&root, ops::CONFIG)
    });
    let org: myco_tui::OrgLoader = Box::new(|| {
        let (views, _warnings) = view::index_universe()?;
        Ok(views
            .into_iter()
            .map(|v| {
                let (items, archived) = v.snap.map(|s| (s.items, s.archived)).unwrap_or_default();
                myco_tui::OrgRepo {
                    name: v.name,
                    worktree: v.worktree.into_std_path_buf(),
                    items,
                    archived,
                    error: v.error,
                }
            })
            .collect())
    });
    myco_tui::run(
        ctx.dir.as_deref(),
        ctx.root.as_deref(),
        ctx.no_commit,
        view,
        Some(org),
    )
    .map(|_| 0)
}

fn main() {
    // Restore conventional-Unix SIGPIPE behavior (die silently on
    // EPIPE) — `println!` would otherwise panic when piped to a
    // reader that closes early (e.g. `myco board | head`).
    sigpipe::reset();
    let matches = registry::command().get_matches();
    let ctx = registry::Ctx {
        json: matches.get_flag("json"),
        no_commit: matches.get_flag("no_commit"),
        dir: matches.get_one::<String>("dir").map(PathBuf::from),
        root: matches.get_one::<String>("root").map(PathBuf::from),
    };
    match registry::dispatch(&ctx, &matches) {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            if ctx.json {
                ops::print_json(&serde_json::json!({"error": e}));
            } else {
                eprintln!("myco: error: {e}");
            }
            std::process::exit(2);
        }
    }
}
