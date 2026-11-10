//! myco — the engine as a library. Everything that used to live
//! in the binary lives here: the verb registry, clap tree, ops,
//! serve/batch, views, continuity. `run()` is the compat CLI's
//! whole body; drivers (`myco` bin, `tamias-driver-myco run`)
//! split it into [`parse`]/[`parse_from`] + [`dispatch`] — argv
//! in, exit code out, verbs callable as `dispatch(&matches)`.
//!
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

pub mod absorb;
pub mod adr;
pub mod agentmd;
pub mod backlog;
pub mod batch;
pub mod board;
pub mod cfgverb;
pub mod check;
pub mod commit;
pub mod confirm;
pub mod continuity;
pub mod digest;
pub mod discover;
pub mod done;
pub mod indexcmd;
pub mod init;
pub mod issue;
pub mod itemcmd;
pub mod items;
pub mod links;
pub mod migrate;
pub mod movecmd;
pub mod next;
pub mod ops;
pub mod org;
pub mod plan;
pub mod proposal;
pub mod redo;
pub mod registry;
pub mod report;
pub mod schema;
pub mod serve;
pub mod status;
pub mod templates;
pub mod todo;
pub mod undo;
pub mod view;

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

/// Process argv → parsed matches. Split from [`dispatch`] so
/// drivers can parse *their* argv (a `run` verb, a re-exec)
/// without owning process args. Clap errors/help still exit the
/// process, same as argv parsing.
pub fn parse() -> clap::ArgMatches {
    registry::command().get_matches()
}

/// Same, from an argv iterator — `parse_from(["myco", "board"])`.
pub fn parse_from<I, T>(itr: I) -> clap::ArgMatches
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    registry::command().get_matches_from(itr)
}

/// The global flags → the handler context.
fn ctx_of(m: &clap::ArgMatches) -> registry::Ctx {
    registry::Ctx {
        json: m.get_flag("json"),
        no_commit: m.get_flag("no_commit"),
        dir: m.get_one::<String>("dir").map(PathBuf::from),
        root: m.get_one::<String>("root").map(PathBuf::from),
    }
}

/// One parsed command line → exit code. The dispatch every
/// driver shares; argv parsing is a caller concern.
pub fn dispatch(m: &clap::ArgMatches) -> i32 {
    let ctx = ctx_of(m);
    match registry::dispatch(&ctx, m) {
        Ok(code) => code,
        Err(e) => {
            if ctx.json {
                ops::print_json(&serde_json::json!({"error": e}));
            } else {
                eprintln!("myco: error: {e}");
            }
            2
        }
    }
}

/// The compat CLI body — parse argv, dispatch, hand back the code.
/// `sigpipe::reset` restores conventional-Unix SIGPIPE behavior
/// (die silently on EPIPE) — `println!` would otherwise panic when
/// piped to a reader that closes early (`myco board | head`).
pub fn run() -> i32 {
    sigpipe::reset();
    dispatch(&parse())
}

/// Same, from an argv iterator — the `run` passthrough entry
/// every driver re-execs (`tamias-driver-myco run <myco argv>`).
pub fn run_from<I, T>(argv: I) -> i32
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    sigpipe::reset();
    dispatch(&parse_from(argv))
}
