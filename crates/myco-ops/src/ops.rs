//! Shared plumbing for the lifecycle verbs — the `project/`
//! scaffold and the auto-commit-per-verb machinery.
//!
//! Every mutating verb ends in one atomic commit on the `project`
//! branch (`--no-commit` or `$MYCO_NO_COMMIT` opts out, e.g. for
//! tests and dry runs). Commits happen only when the worktree
//! actually changed — a no-op verb leaves no noise in the audit
//! trail.

use std::path::{Path, PathBuf};

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

/// Name of the lock file — inside the repo's shared git dir
/// (`<main>/.git/myco.lock`) so it never lands in a ledger commit
/// (`git add -A` can't see it) and covers the `project/` worktree
/// of every checkout of the repo.
const LOCK_FILE: &str = "myco.lock";

/// RAII guard for a worktree lock — an OS advisory lock (`flock`)
/// on the repo's lock file. The lock releases when the guard
/// drops (or the process dies — the OS cleans up).
#[derive(Debug)]
pub struct WorktreeLock {
    _file: std::fs::File,
}

/// The file the lock lives on: `<git-common-dir>/myco.lock` — the
/// common dir is shared by a repo's worktrees and exists before
/// `myco init` creates the worktree, so one path covers init and
/// every verb. A dir outside any repo falls back to a dotfile
/// inside itself (nothing can autocommit there anyway, and the
/// ledger spec ignores dotfiles).
fn lock_path(dir: &Path) -> Result<PathBuf, String> {
    match myco_git::common_dir(dir) {
        Ok(git) => Ok(git.join(LOCK_FILE)),
        Err(_) => {
            if dir.is_dir() {
                Ok(dir.join(".myco.lock"))
            } else {
                Err(format!("{} is not a directory", dir.display()))
            }
        }
    }
}

fn lock_at(dir: &Path, exclusive: bool) -> Result<WorktreeLock, String> {
    let path = lock_path(dir)?;
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .map_err(|e| format!("open {}: {e}", path.display()))?;
    let taken = if exclusive {
        file.try_lock()
    } else {
        file.try_lock_shared()
    };
    match taken {
        Ok(()) => Ok(WorktreeLock { _file: file }),
        Err(std::fs::TryLockError::WouldBlock) => {
            Err("another myco process is writing — retry".into())
        }
        Err(std::fs::TryLockError::Error(e)) => Err(format!("lock {}: {e}", path.display())),
    }
}

/// The exclusive mutation lock — every load→mutate→save→commit
/// verb holds it for the verb's whole life (including the
/// autocommit). Fail-fast on contention: concurrent writers
/// corrupt the ledger (dup ids, last-writer-wins), so the second
/// one gets a retry hint instead.
///
/// `dir` is any path inside the repo — the worktree or the repo
/// root; the lock lands on the shared git dir either way.
pub fn lock_worktree(dir: &Path) -> Result<WorktreeLock, String> {
    lock_at(dir, true)
}

/// The shared read lock — concurrent readers don't block each
/// other, but a reader contending with a writer fails fast
/// rather than rendering a mid-verb ledger.
pub fn lock_shared(dir: &Path) -> Result<WorktreeLock, String> {
    lock_at(dir, false)
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
    if !enabled || !myco_git::has_changes(dir).map_err(|e| e.to_string())? {
        return Ok(false);
    }
    myco_git::commit_all(dir, msg)
        .map_err(|e| format!("git commit failed in {}: {e}", dir.display()))?;
    Ok(true)
}

/// Commit only `paths` inside `dir` — used for repo-root edits
/// (`.gitignore`, `pm/` removal) so an unrelated dirty tree isn't
/// swept into the verb's commit. Returns whether a commit was made.
///
/// Scoping is per *file*, not per hunk: callers must pass only
/// paths myco itself wrote this invocation. If the user had
/// pre-existing uncommitted edits inside one of those files, they
/// land in the commit under myco's message — an accepted
/// limitation (hunk-level staging via `git apply --cached` was
/// judged disproportionate for the managed-file call sites).
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
    myco_git::git_ident(dir, &commit)
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

/// Default pager — `-F` quits instantly under one screen, `-R`
/// keeps ANSI colors, `-S` chops instead of wrapping rows, `-X`
/// leaves output in the scrollback.
const DEFAULT_PAGER: &str = "less -FRSX";

/// Pager program: `$MYCO_PAGER` → `$PAGER` → [`DEFAULT_PAGER`].
/// An explicitly empty value disables paging.
fn pager_program() -> Option<String> {
    for var in ["MYCO_PAGER", "PAGER"] {
        if let Ok(v) = std::env::var(var) {
            return if v.trim().is_empty() { None } else { Some(v) };
        }
    }
    Some(DEFAULT_PAGER.to_string())
}

/// `sh -c <pager>` with `text` on its stdin — the pager inherits
/// our stdout/stderr, so scrolling and colors work as usual.
fn spawn_pager(pager: &str, text: &str) -> std::io::Result<()> {
    use std::io::Write;
    let mut child = std::process::Command::new("sh")
        .args(["-c", pager])
        .stdin(std::process::Stdio::piped())
        .spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        // quitting early (`q`) closes the pipe — with SIGPIPE at
        // its default disposition we die like any unix tool, which
        // is what the user expects; ignore the write error for the
        // SIG_IGN case
        let _ = stdin.write_all(text.as_bytes());
    }
    child.wait().map(|_| ())
}

/// Print `text`, paging it through the user's pager when stdout is
/// a tty — `less -F` exits instantly for one-screen output, so
/// callers can page unconditionally. Non-tty output, a disabled or
/// unspawnable pager all fall back to `println!`.
pub fn print_paged(text: &str) {
    use std::io::IsTerminal;
    if std::io::stdout().is_terminal() {
        if let Some(pager) = pager_program() {
            if spawn_pager(&pager, text).is_ok() {
                return;
            }
        }
    }
    println!("{text}");
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::process::Command;

    #[test]
    fn pager_program_env_precedence_and_disable() {
        temp_env::with_vars(
            [("MYCO_PAGER", None::<&str>), ("PAGER", None::<&str>)],
            || assert_eq!(pager_program().as_deref(), Some("less -FRSX")),
        );
        temp_env::with_var("MYCO_PAGER", Some("cat"), || {
            assert_eq!(pager_program().as_deref(), Some("cat"));
        });
        // empty = explicitly disabled
        temp_env::with_var("MYCO_PAGER", Some("  "), || {
            assert_eq!(pager_program(), None);
        });
        // PAGER wins over nothing but loses to MYCO_PAGER
        temp_env::with_vars(
            [("MYCO_PAGER", Some("most")), ("PAGER", Some("more"))],
            || assert_eq!(pager_program().as_deref(), Some("most")),
        );
    }

    fn git(dir: &Path, args: &[&str]) {
        let st = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .stdout(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(st.success(), "git {args:?}");
    }

    #[test]
    fn lock_serializes_writers_and_reads() {
        let d = tempfile::tempdir().unwrap();
        // first writer takes it; a second writer and a reader
        // both fail fast with the retry hint
        let w = lock_worktree(d.path()).unwrap();
        let err = lock_worktree(d.path()).unwrap_err();
        assert!(err.contains("another myco process is writing"), "{err}");
        let err = lock_shared(d.path()).unwrap_err();
        assert!(err.contains("another myco process"), "{err}");
        // drop releases — a new writer takes over, readers now
        // share, and a third writer fails against the reader
        drop(w);
        let r = lock_shared(d.path()).unwrap();
        let _r2 = lock_shared(d.path()).unwrap();
        assert!(lock_worktree(d.path()).is_err());
        drop((r, _r2));
        assert!(lock_worktree(d.path()).is_ok());
    }

    #[test]
    fn lock_lives_in_the_shared_git_dir() {
        // a repo with a linked project worktree — the lock file
        // must land in <root>/.git so `git add -A` can't sweep it
        // into a ledger commit
        let d = tempfile::tempdir().unwrap();
        git(d.path(), &["init", "-q"]);
        git(d.path(), &["config", "user.email", "t@t"]);
        git(d.path(), &["config", "user.name", "t"]);
        myco_git::create_orphan_branch(d.path(), "project", "init").unwrap();
        git(d.path(), &["worktree", "add", "project", "project"]);
        // locking from the worktree and from the repo root
        // contend — same lock file
        let w = lock_worktree(d.path()).unwrap();
        assert!(lock_worktree(&d.path().join("project")).is_err());
        drop(w);
        assert!(lock_worktree(&d.path().join("project")).is_ok());
        assert!(d.path().join(".git/myco.lock").exists());
        assert!(!d.path().join("project/myco.lock").exists());
        git(d.path(), &["worktree", "remove", "--force", "project"]);
    }

    #[test]
    fn commit_paths_clean_tree_is_noop() {
        let d = tempfile::tempdir().unwrap();
        git(d.path(), &["init", "-q"]);
        git(d.path(), &["config", "user.email", "t@t"]);
        git(d.path(), &["config", "user.name", "t"]);
        // enabled but nothing dirty under the path → false, no commit
        assert!(!commit_paths(d.path(), "msg", &[".gitignore"], true).unwrap());
        // disabled short-circuits before even checking
        assert!(!commit_paths(d.path(), "msg", &[".gitignore"], false).unwrap());
        std::fs::write(d.path().join("other.txt"), "x").unwrap();
        // dirty elsewhere doesn't count — only the named paths do
        assert!(!commit_paths(d.path(), "msg", &[".gitignore"], true).unwrap());
    }
}
