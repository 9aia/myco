//! myco — project management, grown not built. A mycorrhizal
//! network for your agents: tasks, signals, handoffs.
//!
//! Agent-first PM: markdown in `project/` is canonical and
//! human-editable; every deterministic write goes through this CLI
//! (auto-commit per verb, `--json` everywhere). The skill teaches
//! agents the verbs; `myco manifest`/`myco schema` are the
//! self-describing surface.
//!
//! Phase 5 wires the views — `schema`, `status [--prompt]`,
//! `board`, `report`, `index`, `org` — plus the `myco-index(-redb)`
//! derived cache that powers the cross-repo reads.

use std::path::PathBuf;

use clap::{Parser, Subcommand};

mod adr;
mod backlog;
mod board;
mod check;
mod commit;
mod digest;
mod discover;
mod indexcmd;
mod init;
mod issue;
mod itemcmd;
mod items;
mod migrate;
mod ops;
mod org;
mod plan;
mod proposal;
mod report;
mod schema;
mod status;
mod templates;
mod todo;

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
    /// The file/state spec, machine-readable JSON Schema.
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
    /// TODO.md tasks — add / done / defer / list.
    Todo {
        #[command(subcommand)]
        cmd: todo::Cmd,
    },
    /// BACKLOG.md parked items — add / promote / list.
    Backlog {
        #[command(subcommand)]
        cmd: backlog::Cmd,
    },
    /// Tracked issues, one file each under `issues/`.
    Issue {
        #[command(subcommand)]
        cmd: issue::Cmd,
    },
    /// Design proposals under `proposals/` (`P-NNN`).
    Proposal {
        #[command(subcommand)]
        cmd: proposal::Cmd,
    },
    /// Execution plans under `plans/` (`PL-NNN`).
    Plan {
        #[command(subcommand)]
        cmd: plan::Cmd,
    },
    /// Architecture decision records under `adrs/` (`ADR-NNNN`).
    Adr {
        #[command(subcommand)]
        cmd: adr::Cmd,
    },
    /// Kind-agnostic item verbs — show / set.
    Item {
        #[command(subcommand)]
        cmd: itemcmd::Cmd,
    },
    /// Cheap digest of the work ledger. `--prompt` = token-sized.
    Status {
        /// Render for an agent prompt (minimal tokens).
        #[arg(long)]
        prompt: bool,
    },
    /// Kanban of the ledger — Backlog | Open | Claimed | In
    /// Progress | Blocked. `--md` emits a markdown table.
    Board {
        /// Emit a markdown table (for docs/READMEs).
        #[arg(long)]
        md: bool,
    },
    /// Aging + velocity + blocked — how the ledger is trending.
    Report,
    /// The derived index: register repos, query/rebuild snapshots.
    Index {
        #[command(subcommand)]
        cmd: indexcmd::Cmd,
    },
    /// Cross-repo views over the index (auto-rebuilds stale repos).
    Org {
        #[command(subcommand)]
        cmd: org::Cmd,
    },
    /// Print the CLI reference as markdown (docs pipeline).
    #[command(hide = true, name = "gen-cli-docs")]
    GenCliDocs,
}

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
        },
        "verbs": [
            "manifest", "schema",
            "init", "check", "migrate", "commit", "absorb",
            "todo", "backlog", "issue", "proposal", "plan", "adr",
            "item show", "item set",
            "status", "board", "report",
            "index add", "index remove", "index list", "index rebuild",
            "org status", "org board",
            "handoff", "resume", "serve",
        ],
        "implemented": [
            "manifest", "gen-cli-docs", "schema",
            "init", "check", "migrate", "commit",
            "todo", "backlog", "issue", "proposal", "plan", "adr",
            "item show", "item set",
            "status", "board", "report",
            "index add", "index remove", "index list", "index rebuild",
            "org status", "org board",
        ],
        "notes": "markdown in project/ is canonical; every verb is one \
                  atomic commit on the `project` branch; the redb index \
                  under $MYCO_HOME|~/.local/share/myco is a disposable \
                  derived cache",
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
        Cmd::Todo { cmd } => todo::run(cmd, dir, root, json, no_commit),
        Cmd::Backlog { cmd } => backlog::run(cmd, dir, root, json, no_commit),
        Cmd::Issue { cmd } => issue::run(cmd, dir, root, json, no_commit),
        Cmd::Proposal { cmd } => proposal::run(cmd, dir, root, json, no_commit),
        Cmd::Plan { cmd } => plan::run(cmd, dir, root, json, no_commit),
        Cmd::Adr { cmd } => adr::run(cmd, dir, root, json, no_commit),
        Cmd::Item { cmd } => itemcmd::run(cmd, dir, root, json, no_commit),
        Cmd::Schema => schema::run(),
        Cmd::Status { prompt } => status::run(dir, root, prompt, json),
        Cmd::Board { md } => board::run(dir, root, md, json),
        Cmd::Report => report::run(dir, root, json),
        Cmd::Index { cmd } => indexcmd::run(cmd, json),
        Cmd::Org { cmd } => org::run(cmd, json),
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
