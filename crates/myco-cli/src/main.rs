//! myco — project management, grown not built. A mycorrhizal
//! network for your agents: tasks, signals, handoffs.
//!
//! Agent-first PM: markdown in `project/` is canonical and
//! human-editable; every deterministic write goes through this CLI
//! (auto-commit per verb, `--json` everywhere). The skill teaches
//! agents the verbs; `myco manifest`/`myco schema` are the
//! self-describing surface.
//!
//! Phase 3 wires the lifecycle verbs — `init`, `check`, `migrate`,
//! `commit` — on `myco-git` + `myco-store-md`. `schema`/`status`
//! stay stubs until phase 5.

use std::path::PathBuf;

use clap::{Parser, Subcommand};

mod check;
mod commit;
mod discover;
mod init;
mod migrate;
mod ops;

/// myco — agent-first project management for a repo's `project/`
/// worktree.
#[derive(Parser)]
#[command(name = "myco", version, about, long_about = None)]
struct Cli {
    /// Emit JSON (accepted everywhere; default output for machines).
    #[arg(long, global = true)]
    json: bool,

    /// Skip the per-verb commit (also suppressed by $MYCO_NO_COMMIT).
    #[arg(long, global = true)]
    no_commit: bool,

    /// Project worktree to act on, overriding discovery.
    #[arg(long, global = true, value_name = "PATH")]
    dir: Option<PathBuf>,

    /// Repo root to act on, overriding discovery (the worktree is
    /// `<root>/project`).
    #[arg(long, global = true, value_name = "PATH")]
    root: Option<PathBuf>,

    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Capabilities + protocol version — agent self-discovery.
    Manifest,
    /// The file/state spec, machine-readable JSON Schema. [phase 5]
    Schema,
    /// Scaffold `project/`: orphan branch + worktree + .gitignore +
    /// seed files. Idempotent — a valid existing worktree gets its
    /// scaffold verified/repaired.
    Init,
    /// Validate schemas + state machine, repair file/index drift.
    /// Exit 1 with findings; `--fix` applies safe repairs + commits.
    Check {
        /// Repair drift in place instead of only reporting.
        #[arg(long)]
        fix: bool,
    },
    /// Convert legacy ledgers to v2: `pm/` → `project/`, or
    /// normalize files already on the `project` branch.
    Migrate,
    /// Stage + commit worktree changes (conventional message).
    Commit {
        /// Commit message (default: `chore: update project ledger`).
        #[arg(short, long)]
        message: Option<String>,
    },
    /// Cheap digest of the work ledger. `--prompt` = token-sized.
    /// [phase 5]
    Status {
        /// Render for an agent prompt (minimal tokens).
        #[arg(long)]
        prompt: bool,
    },
    /// Print the CLI reference as markdown (docs pipeline).
    #[command(hide = true, name = "gen-cli-docs")]
    GenCliDocs,
}

fn manifest() -> serde_json::Value {
    serde_json::json!({
        "name": "myco",
        "version": env!("CARGO_PKG_VERSION"),
        "protocol": 0,
        "description": env!("CARGO_PKG_DESCRIPTION"),
        "repository": env!("CARGO_PKG_REPOSITORY"),
        "capabilities": [
            "manifest",
            "schema", "init", "check", "migrate", "commit", "absorb",
            "index rebuild", "todo", "backlog", "issue", "proposal",
            "plan", "adr", "item set", "handoff", "resume", "status",
            "board", "report", "index add", "org",
        ],
        "implemented": [
            "manifest", "gen-cli-docs",
            "init", "check", "migrate", "commit",
        ],
        "notes": "markdown in project/ is canonical; every verb is one \
                  atomic commit on the `project` branch",
    })
}

fn main() {
    let cli = Cli::parse();
    let json = cli.json;
    let no_commit = cli.no_commit;
    let dir = cli.dir.clone();
    let root = cli.root.clone();
    let result: Result<i32, String> = match cli.command {
        Cmd::Manifest => {
            ops::print_json(&manifest());
            Ok(0)
        }
        Cmd::GenCliDocs => {
            println!("{}", clap_markdown::help_markdown::<Cli>());
            Ok(0)
        }
        Cmd::Init => init::run(dir, root, json, no_commit),
        Cmd::Check { fix } => check::run(dir, root, fix, json, no_commit),
        Cmd::Migrate => migrate::run(dir, root, json, no_commit),
        Cmd::Commit { message } => commit::run(dir, root, message, json, no_commit),
        Cmd::Schema | Cmd::Status { .. } => {
            let name = match &cli.command {
                Cmd::Schema => "schema",
                Cmd::Status { .. } => "status",
                _ => "verb",
            };
            eprintln!("myco {name}: not implemented yet — see project/proposals/v2.md phases");
            std::process::exit(2);
        }
    };
    match result {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            if json {
                ops::print_json(&serde_json::json!({"error": e}));
            } else {
                eprintln!("myco: error: {e}");
            }
            std::process::exit(2);
        }
    }
}
