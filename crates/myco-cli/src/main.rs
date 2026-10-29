//! myco — project management, grown not built. A mycorrhizal
//! network for your agents: tasks, signals, handoffs.
//!
//! Agent-first PM: markdown in `project/` is canonical and
//! human-editable; every deterministic write goes through this CLI
//! (auto-commit per verb, `--json` everywhere). The skill teaches
//! agents the verbs; `myco manifest`/`myco schema` are the
//! self-describing surface.
//!
//! Phase-1 bootstrap: the crate map and verbs below are stubs ahead
//! of phases 2–7 — see `project/proposals/v2.md`.

use clap::{Parser, Subcommand};

/// myco — agent-first project management for a repo's `project/`
/// worktree.
#[derive(Parser)]
#[command(name = "myco", version, about, long_about = None)]
struct Cli {
    /// Emit JSON (accepted everywhere; default output for machines).
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Capabilities + protocol version — agent self-discovery.
    Manifest,
    /// The file/state spec, machine-readable JSON Schema. [phase 2]
    Schema,
    /// Scaffold `project/`: orphan branch + worktree + .gitignore +
    /// seed files. [phase 3]
    Init,
    /// Validate schemas + state machine, repair file/index drift.
    /// [phase 3]
    Check {
        /// Repair drift in place instead of only reporting.
        #[arg(long)]
        fix: bool,
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
            "schema", "init", "check", "migrate", "absorb",
            "index rebuild", "todo", "backlog", "issue", "proposal",
            "plan", "adr", "item set", "handoff", "resume", "status",
            "board", "report", "index add", "org",
        ],
        "implemented": ["manifest", "gen-cli-docs"],
        "notes": "markdown in project/ is canonical; every verb is one \
                  atomic commit on the `project` branch",
    })
}

fn main() {
    let cli = Cli::parse();
    let _json = cli.json;
    match cli.command {
        Cmd::Manifest => {
            let out = serde_json::to_string_pretty(&manifest()).unwrap_or_else(|_| "{}".into());
            println!("{out}");
        }
        Cmd::GenCliDocs => {
            println!("{}", clap_markdown::help_markdown::<Cli>());
        }
        Cmd::Schema | Cmd::Init | Cmd::Check { .. } | Cmd::Status { .. } => {
            let name = match &cli.command {
                Cmd::Schema => "schema",
                Cmd::Init => "init",
                Cmd::Check { .. } => "check",
                Cmd::Status { .. } => "status",
                _ => "verb",
            };
            eprintln!("myco {name}: not implemented yet — see project/proposals/v2.md phases");
            std::process::exit(2);
        }
    }
}
