//! Shared plumbing for the lifecycle verbs — the `project/`
//! scaffold and the auto-commit-per-verb machinery.
//!
//! Every mutating verb ends in one atomic commit on the `project`
//! branch (`--no-commit` or `$MYCO_NO_COMMIT` opts out, e.g. for
//! tests and dry runs). Commits happen only when the worktree
//! actually changed — a no-op verb leaves no noise in the audit
//! trail.

use std::path::Path;

use camino::Utf8PathBuf;
use myco_core::{BacklogDoc, ProjectMeta, TodoDoc};
use myco_store::{Error as StoreError, Store};
use myco_store_md::MdStore;

/// Dirs `myco init` / `check --fix` scaffold (with `.gitkeep`).
const SCAFFOLD_DIRS: [&str; 5] = ["issues", "proposals", "adrs", "plans", "handoffs"];

/// `MdStore` rooted at a worktree path.
pub fn store_at(worktree: &Path) -> Result<MdStore, String> {
    let root = Utf8PathBuf::from_path_buf(worktree.to_path_buf())
        .map_err(|p| format!("{}: non-utf8 path", p.display()))?;
    Ok(MdStore::new(root))
}

/// Default `index.md` — a human map of the ledger.
pub fn index_template(name: &str) -> String {
    format!(
        "# project/ — {name} work ledger\n\n\
         Markdown here is canonical; `myco` verbs are the write path.\n\n\
         - [TODO](TODO.md) — active task list\n\
         - [BACKLOG](BACKLOG.md) — unshaped ideas\n\
         - [issues/](issues/) — tracked issues, one md file each\n\
         - [proposals/](proposals/) — design proposals (`P-###`)\n\
         - [plans/](plans/) — execution plans (`PL-###`)\n\
         - [adrs/](adrs/) — architecture decision records (`ADR-####`)\n\
         - [handoffs/](handoffs/) — session handoffs (`myco handoff`/`resume`)\n"
    )
}

/// Create every missing scaffold piece in the worktree; existing
/// files are never overwritten. Returns the rel paths created.
pub fn scaffold(store: &MdStore, name: &str) -> Result<Vec<String>, StoreError> {
    let mut created = Vec::new();
    let root = store.root();
    let missing = |rel: &str| !root.join(rel).exists();
    if missing("meta.toml") {
        store.save_meta(&ProjectMeta::new(name))?;
        created.push("meta.toml".to_string());
    }
    if missing("TODO.md") {
        store.save_todo(&TodoDoc::new(name))?;
        created.push("TODO.md".to_string());
    }
    if missing("BACKLOG.md") {
        store.save_backlog(&BacklogDoc::default())?;
        created.push("BACKLOG.md".to_string());
    }
    if missing("index.md") {
        store.save_index(&index_template(name))?;
        created.push("index.md".to_string());
    }
    for dir in SCAFFOLD_DIRS {
        let keep = root.join(dir).join(".gitkeep");
        let empty_dir = match std::fs::read_dir(root.join(dir)) {
            Ok(mut it) => it.next().is_none(),
            Err(_) => true,
        };
        if empty_dir && !keep.exists() {
            std::fs::create_dir_all(root.join(dir))?;
            std::fs::write(&keep, "")?;
            created.push(format!("{dir}/.gitkeep"));
        }
    }
    Ok(created)
}

/// Are verb commits enabled? `--no-commit` or `$MYCO_NO_COMMIT`
/// (any value) suppresses them.
pub fn commits_enabled(no_commit: bool) -> bool {
    !no_commit && std::env::var_os("MYCO_NO_COMMIT").is_none()
}

/// One atomic commit of everything under `dir` — the
/// auto-commit-per-verb path. No commit when disabled or when the
/// tree is clean. Returns whether a commit was made.
pub fn autocommit(dir: &Path, msg: &str, enabled: bool) -> Result<bool, String> {
    if !enabled || !myco_git::has_changes(dir) {
        return Ok(false);
    }
    if myco_git::commit_all(dir, msg) {
        Ok(true)
    } else {
        Err(format!("git commit failed in {}", dir.display()))
    }
}

/// Commit only `paths` inside `dir` — used for repo-root edits
/// (`.gitignore`, `pm/` removal) so an unrelated dirty tree isn't
/// swept into the verb's commit. Returns whether a commit was made.
pub fn commit_paths(dir: &Path, msg: &str, paths: &[&str], enabled: bool) -> Result<bool, String> {
    if !enabled {
        return Ok(false);
    }
    let mut args = vec!["status", "--porcelain", "--"];
    args.extend_from_slice(paths);
    let dirty = myco_git::git_out(dir, &args).map_err(|e| e.to_string())?;
    if dirty.is_empty() {
        return Ok(false);
    }
    let mut add = vec!["add", "--"];
    add.extend_from_slice(paths);
    myco_git::git_out(dir, &add).map_err(|e| e.to_string())?;
    let mut commit = vec!["commit", "-m", msg, "--"];
    commit.extend_from_slice(paths);
    myco_git::git_out(dir, &commit)
        .map_err(|e| e.to_string())
        .map(|_| true)
}

/// Pretty JSON to stdout.
pub fn print_json(v: &serde_json::Value) {
    println!(
        "{}",
        serde_json::to_string_pretty(v).unwrap_or_else(|_| v.to_string())
    );
}
