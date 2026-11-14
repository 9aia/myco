//! Shared plumbing for the lifecycle verbs — the `project/`
//! scaffold and the auto-commit-per-verb machinery.
//!
//! Every mutating verb ends in one atomic commit on the `project`
//! branch (`--no-commit` or `$MYCO_NO_COMMIT` opts out, e.g. for
//! tests and dry runs). Commits happen only when the worktree
//! actually changed — a no-op verb leaves no noise in the audit
//! trail.

use std::path::{Path, PathBuf};

use myco_core::{Actor, BacklogDoc, ProjectMeta, TodoDoc};
use myco_store::{Error as StoreError, Store};

/// Dirs `myco init` / `check --fix` scaffold (with `.gitkeep`).
const SCAFFOLD_DIRS: [&str; 5] = ["issues", "proposals", "adrs", "plans", "handoffs"];

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
    match crate::vcs::common_dir(dir) {
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
         - [DONE](DONE.md) — done tasks archive\n\
         - [issues/](issues/) — tracked issues, one md file each\n\
         - [proposals/](proposals/) — design proposals (`P-###`)\n\
         - [plans/](plans/) — execution plans (`PL-###`)\n\
         - [adrs/](adrs/) — architecture decision records (`ADR-####`)\n\
         - [handoffs/](handoffs/) — session handoffs (`myco handoff`/`resume`)\n"
    )
}

/// Create every missing scaffold piece in the worktree; existing
/// files are never overwritten. `root` is the store's worktree
/// dir (the `.gitkeep` sweep is raw fs — the port has no "list
/// dir" verb). Returns the rel paths created.
pub fn scaffold(store: &impl Store, root: &Path, name: &str) -> Result<Vec<String>, StoreError> {
    let mut created = Vec::new();
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
    if missing("DONE.md") {
        store.save_done(&myco_core::DoneDoc::default())?;
        created.push("DONE.md".to_string());
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

/// Fallback actor for the `Actor:` commit trailer and every
/// actor default — the local CLI wrote it. The last rung of
/// [`resolve_actor`]'s chain; verbs that name an actor
/// (`--actor`/`--owner`) resolve through the chain instead.
pub const DEFAULT_ACTOR: &str = "cli";

/// Sanitize a `user.name` into actor-id shape — actor ids allow
/// `[A-Za-z0-9._:-]`; anything else collapses to `-` so "Luis P."
/// becomes `human:Luis-P.` rather than failing validation.
fn sanitize_actor_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut dash = false;
    for c in name.trim().chars() {
        if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
            out.push(c);
            dash = false;
        } else if !dash {
            out.push('-');
            dash = true;
        }
    }
    out.trim_matches('-').to_string()
}

/// The actor-resolution chain every verb shares — explicit flag
/// first, then environment, then git identity, then the CLI
/// fallback:
///
/// 1. `explicit` — the verb's `--actor`/`--owner` value.
/// 2. `$MYCO_ACTOR` — the agent/hook-set identity (invalid values
///    fall through rather than fail).
/// 3. `human:<git user.name>` — the repo's configured identity,
///    sanitized into actor-id shape. `dir` is any path inside the
///    repo (the project worktree works); outside a repo or with
///    no `user.name` this rung skips.
/// 4. [`DEFAULT_ACTOR`] — `cli`.
///
/// There is no session-env rung: sepia sessions reach myco as
/// `--session` values on the continuity verbs, not as an actor
/// convention — nothing in the org sets one.
pub fn resolve_actor(dir: Option<&Path>, explicit: Option<&Actor>) -> Actor {
    if let Some(a) = explicit {
        return a.clone();
    }
    if let Ok(v) = std::env::var("MYCO_ACTOR") {
        if let Ok(a) = Actor::new(v.trim()) {
            return a;
        }
    }
    if let Some(d) = dir {
        if let Ok(name) = crate::vcs::git_out(d, &["config", "user.name"]) {
            let sanitized = sanitize_actor_name(name.trim());
            if !sanitized.is_empty() {
                if let Ok(a) = Actor::new(&format!("human:{sanitized}")) {
                    return a;
                }
            }
        }
    }
    Actor::new(DEFAULT_ACTOR).unwrap_or_else(|_| unreachable!("DEFAULT_ACTOR is a valid actor id"))
}

/// `msg` + the `Actor:` trailer — every myco commit names the
/// verb's actor in the message body (T-011: actor-tagged
/// trailers, not per-actor git authors — author/committer stay
/// the repo's stable `user.name`/`user.email` identity, or the
/// `myco@localhost` fallback). `None` resolves to
/// [`DEFAULT_ACTOR`]. The subject line is untouched, so `%s`
/// parses (e.g. `report`'s velocity walk) never see it.
pub fn commit_msg(msg: &str, actor: Option<&Actor>) -> String {
    let actor = actor.map(Actor::as_str).unwrap_or(DEFAULT_ACTOR);
    format!("{msg}\n\nActor: {actor}")
}

/// Mutating verbs must not write a ledger they cannot commit. A
/// detached `project/` worktree is the failure mode that costs the
/// most: every write lands, no branch records it, and the audit
/// trail is gone the moment the worktree moves. `check` reports and
/// repairs it; every write path refuses it first.
pub fn ensure_committable(dir: &Path) -> Result<(), String> {
    if crate::vcs::is_detached(dir) == Some(true) {
        return Err(format!(
            "{} is on a detached HEAD — refusing to write a ledger whose commits would be \
             unreachable; run `myco check --fix` to reattach it",
            dir.display()
        ));
    }
    Ok(())
}

/// Refuse a mutation while the ledger branch is behind its
/// tracked remote — every verb commit would deepen the divergence
/// `myco sync` exists to close.
///
/// `auto_fetch` (`sync.auto_fetch`, default on) runs a
/// best-effort `git fetch origin <branch>` first: the cached
/// `refs/remotes/origin/<branch>` can only *under*-report
/// divergence, so refreshing it before the check is what makes
/// the gate honest. A failed fetch (offline, no remote, bad
/// creds) is never fatal — the check falls back to the stale
/// ref exactly as before. `false` skips the fetch entirely.
/// Untracked branches (never pushed, no remote) pass — pushing is
/// a repo-level choice, not this verb's business.
pub fn ensure_synced(dir: &Path, auto_fetch: bool) -> Result<(), String> {
    let Some(branch) = crate::vcs::worktree_branch(dir) else {
        return Ok(());
    };
    if auto_fetch {
        let _ = crate::vcs::fetch_branch(dir, "origin", &branch);
    }
    if !crate::vcs::tracks_branch(dir, "origin", &branch) {
        return Ok(());
    }
    let remote = format!("refs/remotes/origin/{branch}");
    let Some(behind) = crate::vcs::commits_behind(dir, &remote) else {
        return Ok(());
    };
    if behind > 0 {
        return Err(format!(
            "the ledger is {behind} commit(s) behind origin/{branch} — run `myco sync` \
             first so mutations land on the published tip"
        ));
    }
    Ok(())
}

/// One atomic commit of everything under `dir` — the
/// auto-commit-per-verb path. `msg` gains the `Actor:` trailer
/// ([`commit_msg`]; `actor` is the verb's actor, `None` →
/// [`resolve_actor`] over `dir` — `$MYCO_ACTOR` → `human:<git
/// user.name>` → `cli`). No commit when disabled or when the
/// tree is clean. A landed commit on the `project` branch then
/// gets the best-effort [`maybe_push`] publish (`sync.auto_push`,
/// default on). Returns whether a commit was made.
pub fn autocommit(
    dir: &Path,
    msg: &str,
    actor: Option<&Actor>,
    enabled: bool,
) -> Result<bool, String> {
    if !enabled || !crate::vcs::has_changes(dir).map_err(|e| e.to_string())? {
        return Ok(false);
    }
    let resolved;
    let actor = match actor {
        Some(a) => a,
        None => {
            resolved = resolve_actor(Some(dir), None);
            &resolved
        }
    };
    crate::vcs::commit_all(dir, &commit_msg(msg, Some(actor)))
        .map_err(|e| format!("git commit failed in {}: {e}", dir.display()))?;
    if let Some(w) = maybe_push(dir) {
        eprintln!("myco: warning: {w}");
    }
    Ok(true)
}

/// Best-effort publish of the ledger branch after a commit —
/// `[sync] auto_push` (default on; `false` opts out). Only fires
/// on the `project` branch: a `--dir` ledger parked on another
/// branch (or a plain dir with no checkout) stays local —
/// auto-push must never publish a branch the ledger doesn't own.
/// The config read is the shared TOML loader every front-end
/// binds; missing/broken config means `true`. A repo with no
/// `origin` remote skips silently — local-only ledgers don't earn
/// a warning per mutation. Returns the warning on push failure —
/// never an error: the commit already landed, pushing is advisory
/// (mirrors `auto_fetch`, which never blocks either).
pub fn maybe_push(dir: &Path) -> Option<String> {
    if crate::vcs::worktree_branch(dir).as_deref() != Some(crate::discover::BRANCH) {
        return None;
    }
    let root = crate::vcs::main_worktree(dir).unwrap_or_else(|| dir.to_path_buf());
    if !crate::config::sync_auto_push_for(&root, crate::config::TOML) {
        return None;
    }
    // no `origin` at all → nothing to publish to; skip silently
    // rather than warning on every mutation of a local-only repo
    if crate::vcs::git_out(dir, &["remote", "get-url", "origin"]).is_err() {
        return None;
    }
    crate::vcs::push_branch(dir, "origin", None)
        .err()
        .map(|e| format!("auto-push of {} failed: {e}", crate::discover::BRANCH))
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
pub fn commit_paths(
    dir: &Path,
    msg: &str,
    paths: &[&str],
    actor: Option<&Actor>,
    enabled: bool,
) -> Result<bool, String> {
    if !enabled {
        return Ok(false);
    }
    let mut args = vec!["status", "--porcelain", "--"];
    args.extend_from_slice(paths);
    let dirty = crate::vcs::git_out(dir, &args).map_err(|e| e.to_string())?;
    if dirty.is_empty() {
        return Ok(false);
    }
    let mut add = vec!["add", "--"];
    add.extend_from_slice(paths);
    crate::vcs::git_out(dir, &add).map_err(|e| e.to_string())?;
    let resolved;
    let actor = match actor {
        Some(a) => a,
        None => {
            resolved = resolve_actor(Some(dir), None);
            &resolved
        }
    };
    let msg = commit_msg(msg, Some(actor));
    let mut commit = vec!["commit", "-m", msg.as_str(), "--"];
    commit.extend_from_slice(paths);
    crate::vcs::git_ident(dir, &commit)
        .map_err(|e| e.to_string())
        .map(|_| true)
}

/// What [`undo_last`] reverted — the `undo` verb's report payload.
#[derive(Debug)]
pub struct UndoOutcome {
    /// The commit that was reset away.
    pub hash: String,
    /// Its subject line.
    pub subject: String,
    /// Its author date (`%as`).
    pub date: String,
    /// Caveats — e.g. multi-commit verbs leave sibling commits a
    /// worktree-scoped reset can't reach.
    pub warnings: Vec<String>,
}

/// Subjects of verbs that also commit *outside* this worktree —
/// `init`/`check --fix`/`migrate` land legs on the main branch,
/// `item move` commits in a second repo. Undoing their
/// project-side tip can't revert the whole verb; the caller gets
/// a warning instead of a silent half-undo.
const MULTI_COMMIT_SUBJECTS: &[&str] = &[
    "chore: init",
    "chore: migrate",
    "chore: move",
    "chore: check --fix",
];

/// `myco undo` — move the worktree's branch tip back one commit
/// (`git reset --hard HEAD~1`). Every mutating verb is exactly one
/// commit on the ledger branch, so the reset *is* the inverse
/// mutation — including file deletions (`todo done`'s removed
/// line comes back whole). The commit stays reachable through the
/// reflog; nothing is garbage-collected.
///
/// Guards, in order: the caller holds [`lock_worktree`]; the
/// checkout must not be detached ([`ensure_committable`]); the
/// worktree must be clean — `--no-commit` writes are uncommitted
/// ledger state a hard reset would silently destroy; `HEAD~1`
/// must exist (the orphan root has nothing behind it); and a tip
/// already published on `origin` is refused unless `force` —
/// rewriting pushed audit-trail history is the one real hazard.
///
/// `dir` is the `project/` worktree (or any dir inside it).
pub fn undo_last(dir: &Path, force: bool) -> Result<UndoOutcome, String> {
    ensure_committable(dir)?;
    let entry = crate::vcs::log(dir, Some(1))
        .map_err(|e| e.to_string())?
        .into_iter()
        .next()
        .ok_or_else(|| "nothing to undo — the ledger branch has no commits".to_string())?;
    let base = crate::vcs::git_out(dir, &["rev-parse", "--verify", "HEAD~1"])
        .map_err(|_| format!("nothing to undo — {} is the root commit", entry.hash))?;
    if crate::vcs::has_changes(dir).map_err(|e| e.to_string())? {
        return Err(
            "uncommitted changes in the worktree — `myco commit` them or clean the tree first; \
             undo only rewinds committed state"
                .into(),
        );
    }
    if let Some(branch) = crate::vcs::worktree_branch(dir) {
        let remote = format!("refs/remotes/origin/{branch}");
        let published = crate::vcs::tracks_branch(dir, "origin", &branch)
            && crate::vcs::commits_ahead(dir, &remote) == Some(0);
        if published && !force {
            return Err(format!(
                "the tip commit is already published on origin/{branch} — undo would rewrite \
                 pushed history; pass --force to override"
            ));
        }
    }
    crate::vcs::git_out(dir, &["reset", "--hard", "HEAD~1"])
        .map_err(|e| format!("git reset failed in {}: {e}", dir.display()))?;
    let mut warnings = multi_commit_warnings(&entry.subject);
    // Record the undone commit for `redo`. The journal self-
    // invalidates: if its top anchor isn't the HEAD we just left,
    // the timeline moved and the stale stack is dropped first.
    let mut j = crate::history::load(dir);
    if !crate::history::top_valid(&j, &entry.hash) {
        crate::history::clear(&mut j);
    }
    crate::history::push(
        &mut j,
        crate::history::HistoryEntry {
            target: entry.hash.clone(),
            base: base.trim().to_string(),
            subject: entry.subject.clone(),
            date: entry.date.clone(),
        },
    );
    if let Err(e) = crate::history::save(dir, &j) {
        warnings.push(format!(
            "undo committed but the redo journal could not be saved ({e}) — redo may be unavailable"
        ));
    }
    Ok(UndoOutcome {
        hash: entry.hash,
        subject: entry.subject,
        date: entry.date,
        warnings,
    })
}

/// `myco redo` — restore the commit the last undo moved away
/// from: pop the top of the undo journal
/// (`<git-common-dir>/myco-history.json`, see
/// [`crate::history`]) and `git reset --hard` to its target.
/// Repeatable — each pop exposes the next-older undone commit,
/// which is the redo stack single-step reflog tricks can't
/// express.
///
/// Guards mirror undo's: committable checkout, clean worktree (a
/// hard reset would destroy `--no-commit` writes), and the top
/// entry's `base` must still equal HEAD — any commit, pull, or
/// manual git op since the undo breaks the anchor and the stack
/// self-clears. No published-tip guard: re-applying a commit
/// that was already the tip rewrites nothing new.
pub fn redo_last(dir: &Path) -> Result<UndoOutcome, String> {
    ensure_committable(dir)?;
    let head = crate::history::head_sha(dir)
        .map_err(|_| "nothing to redo — the ledger branch has no commits".to_string())?;
    let mut j = crate::history::load(dir);
    let Some(top) = j.future.last().cloned() else {
        return Err("nothing to redo — no undone commit is waiting".into());
    };
    if top.base != head {
        crate::history::clear(&mut j);
        let _ = crate::history::save(dir, &j);
        return Err("nothing to redo — the ledger moved since the last undo".into());
    }
    if crate::vcs::has_changes(dir).map_err(|e| e.to_string())? {
        return Err(
            "uncommitted changes in the worktree — `myco commit` them or clean the tree first; \
             redo only rewinds committed state"
                .into(),
        );
    }
    if crate::vcs::git_out(
        dir,
        &[
            "rev-parse",
            "--verify",
            &format!("{}^{{commit}}", top.target),
        ],
    )
    .is_err()
    {
        return Err(format!(
            "nothing to redo — {} no longer exists",
            &top.target[..12]
        ));
    }
    crate::vcs::git_out(dir, &["reset", "--hard", &top.target])
        .map_err(|e| format!("git reset failed in {}: {e}", dir.display()))?;
    j.future.pop();
    let mut warnings = multi_commit_warnings(&top.subject);
    if let Err(e) = crate::history::save(dir, &j) {
        warnings.push(format!(
            "redo committed but the redo journal could not be saved ({e}) — further redo may be unavailable"
        ));
    }
    Ok(UndoOutcome {
        hash: top.target,
        subject: top.subject,
        date: top.date,
        warnings,
    })
}

/// How many commits `redo` could still restore — 0 when the
/// journal is empty or its top anchor no longer matches HEAD.
pub fn redo_depth(dir: &Path) -> usize {
    match crate::history::head_sha(dir) {
        Ok(head) => crate::history::depth(dir, &head),
        Err(_) => 0,
    }
}

/// The redoable stack, next-redo first — backs `myco undo --list`.
pub fn history_list(dir: &Path) -> Vec<crate::history::HistoryEntry> {
    crate::history::list(dir)
}

/// Sibling-leg warnings for verbs that commit outside the
/// worktree (`init`, `migrate`, `move`, `check --fix`).
fn multi_commit_warnings(subject: &str) -> Vec<String> {
    MULTI_COMMIT_SUBJECTS
        .iter()
        .filter(|p| subject.starts_with(**p))
        .map(|p| format!("{p}… also commits outside this worktree — those sibling commits remain"))
        .collect()
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
/// `$MYCO_PAGER`/`$PAGER` are executed verbatim (git-parity
/// behavior) — an env var is a trusted setting, never user data.
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
        crate::vcs::create_orphan_branch(d.path(), "project", "init").unwrap();
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
        assert!(!commit_paths(d.path(), "msg", &[".gitignore"], None, true).unwrap());
        // disabled short-circuits before even checking
        assert!(!commit_paths(d.path(), "msg", &[".gitignore"], None, false).unwrap());
        std::fs::write(d.path().join("other.txt"), "x").unwrap();
        // dirty elsewhere doesn't count — only the named paths do
        assert!(!commit_paths(d.path(), "msg", &[".gitignore"], None, true).unwrap());
    }

    #[test]
    fn commit_msg_appends_actor_trailer() {
        // no named actor → the cli default
        assert_eq!(commit_msg("chore: x", None), "chore: x\n\nActor: cli");
        let a = Actor::new("devin").unwrap();
        assert_eq!(commit_msg("chore: y", Some(&a)), "chore: y\n\nActor: devin");
        let h = Actor::new("human:luis").unwrap();
        assert_eq!(
            commit_msg("chore: z", Some(&h)),
            "chore: z\n\nActor: human:luis"
        );
    }

    #[test]
    fn resolve_actor_chain() {
        let d = tempfile::tempdir().unwrap();
        git(d.path(), &["init", "-q"]);
        git(d.path(), &["config", "user.email", "t@t"]);
        git(d.path(), &["config", "user.name", "Luis P."]);
        // explicit wins over everything
        let devin = Actor::new("devin").unwrap();
        temp_env::with_vars([("MYCO_ACTOR", Some("agent-x"))], || {
            assert_eq!(resolve_actor(Some(d.path()), Some(&devin)), devin);
            // MYCO_ACTOR beats the git rung
            assert_eq!(
                resolve_actor(Some(d.path()), None),
                Actor::new("agent-x").unwrap()
            );
        });
        // invalid MYCO_ACTOR falls through to the git identity —
        // sanitized into actor-id shape
        temp_env::with_var("MYCO_ACTOR", Some("not an actor!!"), || {
            assert_eq!(
                resolve_actor(Some(d.path()), None),
                Actor::new("human:Luis-P.").unwrap()
            );
        });
        // no env, no repo → the cli fallback
        temp_env::with_vars([("MYCO_ACTOR", None::<&str>)], || {
            assert_eq!(
                resolve_actor(None, None),
                Actor::new(DEFAULT_ACTOR).unwrap()
            );
        });
    }

    #[test]
    fn autocommit_stamps_actor_trailer() {
        let d = tempfile::tempdir().unwrap();
        git(d.path(), &["init", "-q"]);
        git(d.path(), &["config", "user.email", "t@t"]);
        git(d.path(), &["config", "user.name", "t"]);
        let body = || {
            String::from_utf8(
                Command::new("git")
                    .arg("-C")
                    .arg(d.path())
                    .args(["log", "-1", "--format=%B"])
                    .output()
                    .unwrap()
                    .stdout,
            )
            .unwrap()
        };
        // a verb's named actor lands in the body, not the subject
        let devin = Actor::new("devin").unwrap();
        std::fs::write(d.path().join("a.md"), "x").unwrap();
        assert!(autocommit(d.path(), "chore: first", Some(&devin), true).unwrap());
        let b = body();
        assert!(b.contains("Actor: devin"), "{b}");
        assert_eq!(b.lines().next().unwrap(), "chore: first");
        // no actor → the resolved chain (`human:<git user.name>`
        // here — the repo sets `user.name = t`); git author stays
        // the repo identity
        std::fs::write(d.path().join("b.md"), "x").unwrap();
        assert!(autocommit(d.path(), "chore: second", None, true).unwrap());
        let b = body();
        assert!(b.contains("Actor: human:t"), "{b}");
        let author = String::from_utf8(
            Command::new("git")
                .arg("-C")
                .arg(d.path())
                .args(["log", "-1", "--format=%an"])
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap();
        assert_eq!(author.trim(), "t");
    }

    /// A plain git repo on `main` with `n` commits of `f{i}.txt`.
    fn repo(n: usize) -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        git(d.path(), &["init", "-q", "-b", "main"]);
        git(d.path(), &["config", "user.email", "t@t"]);
        git(d.path(), &["config", "user.name", "t"]);
        for i in 0..n {
            std::fs::write(d.path().join(format!("f{i}.txt")), "x").unwrap();
            git(d.path(), &["add", "-A"]);
            git(d.path(), &["commit", "-qm", &format!("chore: c{i}")]);
        }
        d
    }

    fn head(dir: &Path) -> String {
        String::from_utf8(
            Command::new("git")
                .arg("-C")
                .arg(dir)
                .args(["rev-parse", "HEAD"])
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap()
        .trim()
        .to_string()
    }

    #[test]
    fn undo_rewinds_the_tip_one_commit() {
        let d = repo(3);
        let before = head(d.path());
        let out = undo_last(d.path(), false).unwrap();
        assert_eq!(out.subject, "chore: c2");
        assert_ne!(head(d.path()), before);
        // the reset is a hard rewind — the tip's file is gone,
        // the parent commit's survives
        assert!(!d.path().join("f2.txt").exists());
        assert!(d.path().join("f1.txt").exists());
        // nothing is lost — the undone commit stays in the reflog
        let reflog = String::from_utf8(
            Command::new("git")
                .arg("-C")
                .arg(d.path())
                .args(["log", "-g", "--format=%s"])
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap();
        assert!(reflog.contains("chore: c2"), "{reflog}");
    }

    #[test]
    fn undo_guards() {
        // no commits at all
        let d = tempfile::tempdir().unwrap();
        git(d.path(), &["init", "-q", "-b", "main"]);
        assert!(undo_last(d.path(), false)
            .unwrap_err()
            .contains("no commits"));
        // the orphan root has nothing behind it
        let d = repo(1);
        assert!(undo_last(d.path(), false)
            .unwrap_err()
            .contains("root commit"));
        // uncommitted --no-commit writes would die in the reset
        let d = repo(2);
        std::fs::write(d.path().join("f0.txt"), "dirty").unwrap();
        assert!(undo_last(d.path(), false)
            .unwrap_err()
            .contains("uncommitted changes"));
        // a multi-commit verb warns — the sibling legs remain
        let d = repo(2);
        git(d.path(), &["commit", "-q", "--amend", "-m", "chore: init"]);
        let out = undo_last(d.path(), false).unwrap();
        assert_eq!(out.warnings.len(), 1, "{:?}", out.warnings);
        assert!(out.warnings[0].contains("outside this worktree"));
    }

    #[test]
    fn undo_refuses_a_published_tip_unless_forced() {
        let d = repo(2);
        let remote = tempfile::tempdir().unwrap();
        git(remote.path(), &["init", "-q", "--bare"]);
        git(
            d.path(),
            &["remote", "add", "origin", remote.path().to_str().unwrap()],
        );
        git(d.path(), &["push", "-qu", "origin", "main"]);
        let err = undo_last(d.path(), false).unwrap_err();
        assert!(err.contains("published"), "{err}");
        // --force rewinds anyway — the caller owns the hazard
        undo_last(d.path(), true).unwrap();
        assert!(!d.path().join("f1.txt").exists());
    }

    #[test]
    fn redo_restores_the_undone_commit() {
        let d = repo(2);
        let tip = head(d.path());
        undo_last(d.path(), false).unwrap();
        assert!(!d.path().join("f1.txt").exists());
        let out = redo_last(d.path()).unwrap();
        assert_eq!(out.subject, "chore: c1");
        assert_eq!(head(d.path()), tip);
        assert!(d.path().join("f1.txt").exists());
        // a second redo refuses — the journal is drained
        assert!(redo_last(d.path()).unwrap_err().contains("nothing to redo"));
        // …and undo still works after a redo — they ping-pong
        undo_last(d.path(), false).unwrap();
        assert!(!d.path().join("f1.txt").exists());
    }

    #[test]
    fn redo_walks_the_stack_multi_level() {
        // the redo stack reflog tricks can't express: undo twice,
        // redo twice — each pop restores the exact next commit
        let d = repo(4);
        let h3 = head(d.path());
        let h2 = {
            // c2's hash — capture before rewinding
            let out = String::from_utf8(
                Command::new("git")
                    .arg("-C")
                    .arg(d.path())
                    .args(["rev-parse", "HEAD~1"])
                    .output()
                    .unwrap()
                    .stdout,
            )
            .unwrap();
            out.trim().to_string()
        };
        let _ = h3;
        undo_last(d.path(), false).unwrap(); // c3 away
        undo_last(d.path(), false).unwrap(); // c2 away
        assert_eq!(redo_depth(d.path()), 2);
        let out = redo_last(d.path()).unwrap();
        assert_eq!(out.hash, h2, "first redo restores the newest undo");
        assert_eq!(redo_depth(d.path()), 1);
        redo_last(d.path()).unwrap();
        assert_eq!(head(d.path()), h3, "second redo reaches the tip");
        assert_eq!(redo_depth(d.path()), 0);
        assert!(redo_last(d.path()).unwrap_err().contains("nothing to redo"));
        // the journal lives in the git common dir — never committed
        assert!(d.path().join(".git/myco-history.json").exists());
    }

    #[test]
    fn redo_invalidates_when_the_ledger_moves() {
        let d = repo(3);
        undo_last(d.path(), false).unwrap();
        undo_last(d.path(), false).unwrap();
        assert_eq!(redo_depth(d.path()), 2);
        // any commit breaks every anchor — the whole stack clears
        std::fs::write(d.path().join("f9.txt"), "x").unwrap();
        git(d.path(), &["add", "-A"]);
        git(d.path(), &["commit", "-qm", "chore: new"]);
        assert_eq!(redo_depth(d.path()), 0);
        assert!(redo_last(d.path()).unwrap_err().contains("nothing to redo"));
        // …and a fresh undo starts a new stack
        undo_last(d.path(), false).unwrap();
        assert_eq!(redo_depth(d.path()), 1);
    }

    #[test]
    fn corrupt_journal_self_heals() {
        let d = repo(2);
        // garbage where the journal should be — reads as empty,
        // never an error
        std::fs::write(d.path().join(".git/myco-history.json"), "{not json").unwrap();
        assert!(redo_last(d.path()).unwrap_err().contains("nothing to redo"));
        assert_eq!(redo_depth(d.path()), 0);
        // undo still works and rewrites a valid journal
        undo_last(d.path(), false).unwrap();
        assert_eq!(redo_depth(d.path()), 1);
        redo_last(d.path()).unwrap();
    }

    #[test]
    fn undo_after_redo_rebuilds_the_stack() {
        let d = repo(3);
        undo_last(d.path(), false).unwrap();
        undo_last(d.path(), false).unwrap();
        redo_last(d.path()).unwrap(); // one level restored
        assert_eq!(redo_depth(d.path()), 1);
        // undoing the just-restored tip pushes a fresh entry on
        // top of the surviving stack — undo↔redo stays symmetric
        undo_last(d.path(), false).unwrap();
        assert_eq!(redo_depth(d.path()), 2);
        redo_last(d.path()).unwrap();
        redo_last(d.path()).unwrap();
        assert_eq!(redo_depth(d.path()), 0);
        assert!(redo_last(d.path()).unwrap_err().contains("nothing to redo"));
    }

    #[test]
    fn redo_guards() {
        let d = repo(2);
        // no undo happened — the last action is a commit
        assert!(redo_last(d.path()).unwrap_err().contains("nothing to redo"));
        undo_last(d.path(), false).unwrap();
        // uncommitted writes would die in the reset
        std::fs::write(d.path().join("dirty.txt"), "x").unwrap();
        assert!(redo_last(d.path())
            .unwrap_err()
            .contains("uncommitted changes"));
        // a commit since the undo breaks the journal anchor
        std::fs::remove_file(d.path().join("dirty.txt")).unwrap();
        std::fs::write(d.path().join("f2.txt"), "x").unwrap();
        git(d.path(), &["add", "-A"]);
        git(d.path(), &["commit", "-qm", "chore: new"]);
        assert!(redo_last(d.path()).unwrap_err().contains("nothing to redo"));
    }

    /// A repo with a bare `origin`, the orphan `project` branch
    /// checked out at `<root>/project`, and `myco.toml` contents
    /// written at the root (uncommitted — config reads the
    /// worktree, not git).
    fn ledger_repo(myco_toml: Option<&str>) -> (tempfile::TempDir, tempfile::TempDir, PathBuf) {
        let d = tempfile::tempdir().unwrap();
        git(d.path(), &["init", "-q"]);
        git(d.path(), &["config", "user.email", "t@t"]);
        git(d.path(), &["config", "user.name", "t"]);
        let bare = tempfile::tempdir().unwrap();
        git(bare.path(), &["init", "-q", "--bare"]);
        git(
            d.path(),
            &["remote", "add", "origin", bare.path().to_str().unwrap()],
        );
        crate::vcs::create_orphan_branch(d.path(), "project", "init").unwrap();
        git(d.path(), &["worktree", "add", "project", "project"]);
        if let Some(t) = myco_toml {
            std::fs::write(d.path().join("myco.toml"), t).unwrap();
        }
        let wt = d.path().join("project");
        (d, bare, wt)
    }

    fn bare_has(bare: &Path, branch: &str) -> bool {
        Command::new("git")
            .arg("-C")
            .arg(bare)
            .args(["rev-parse", "--verify", &format!("refs/heads/{branch}")])
            .stdout(std::process::Stdio::null())
            .status()
            .unwrap()
            .success()
    }

    #[test]
    fn autocommit_auto_push_publishes_the_ledger_branch() {
        // no myco.toml → the default (on) publishes the commit
        let (_d, bare, wt) = ledger_repo(None);
        let home = tempfile::tempdir().unwrap();
        temp_env::with_vars(
            [
                ("MYCO_CONFIG", None::<&str>),
                ("XDG_CONFIG_HOME", None::<&str>),
                ("HOME", Some(home.path().to_string_lossy().as_ref())),
            ],
            || {
                assert!(!bare_has(bare.path(), "project"));
                std::fs::write(wt.join("TODO.md"), "# TODO\n").unwrap();
                assert!(autocommit(&wt, "chore: first", None, true).unwrap());
                assert!(bare_has(bare.path(), "project"));
                // the published tip IS the commit autocommit made
                let remote = String::from_utf8(
                    Command::new("git")
                        .arg("-C")
                        .arg(bare.path())
                        .args(["rev-parse", "refs/heads/project"])
                        .output()
                        .unwrap()
                        .stdout,
                )
                .unwrap();
                assert_eq!(remote.trim(), head(&wt));
            },
        );
    }

    #[test]
    fn autocommit_auto_push_opts_out() {
        // `auto_push = false` → the commit lands locally, the
        // remote never sees the branch
        let (_d, bare, wt) = ledger_repo(Some("[sync]\nauto_push = false\n"));
        let home = tempfile::tempdir().unwrap();
        temp_env::with_vars(
            [
                ("MYCO_CONFIG", None::<&str>),
                ("XDG_CONFIG_HOME", None::<&str>),
                ("HOME", Some(home.path().to_string_lossy().as_ref())),
            ],
            || {
                std::fs::write(wt.join("TODO.md"), "# TODO\n").unwrap();
                assert!(autocommit(&wt, "chore: first", None, true).unwrap());
                assert!(!bare_has(bare.path(), "project"));
            },
        );
    }

    #[test]
    fn autocommit_auto_push_never_publishes_other_branches() {
        // auto_push on (explicit), but the ledger sits on `main` (a --dir
        // edge) — the gate is the `project` branch, not "whatever
        // is checked out"
        let d = repo(1);
        let bare = tempfile::tempdir().unwrap();
        git(bare.path(), &["init", "-q", "--bare"]);
        git(
            d.path(),
            &["remote", "add", "origin", bare.path().to_str().unwrap()],
        );
        std::fs::write(d.path().join("myco.toml"), "[sync]\nauto_push = true\n").unwrap();
        let home = tempfile::tempdir().unwrap();
        temp_env::with_vars(
            [
                ("MYCO_CONFIG", None::<&str>),
                ("XDG_CONFIG_HOME", None::<&str>),
                ("HOME", Some(home.path().to_string_lossy().as_ref())),
            ],
            || {
                std::fs::write(d.path().join("f9.txt"), "x").unwrap();
                assert!(autocommit(d.path(), "chore: x", None, true).unwrap());
                assert!(!bare_has(bare.path(), "main"));
            },
        );
    }

    #[test]
    fn maybe_push_skips_a_repo_without_origin() {
        // default-on but no `origin` remote → silent skip, not a
        // warning on every mutation of a local-only repo
        let d = tempfile::tempdir().unwrap();
        git(d.path(), &["init", "-q"]);
        git(d.path(), &["config", "user.email", "t@t"]);
        git(d.path(), &["config", "user.name", "t"]);
        crate::vcs::create_orphan_branch(d.path(), "project", "init").unwrap();
        git(d.path(), &["worktree", "add", "project", "project"]);
        let wt = d.path().join("project");
        let home = tempfile::tempdir().unwrap();
        temp_env::with_vars(
            [
                ("MYCO_CONFIG", None::<&str>),
                ("XDG_CONFIG_HOME", None::<&str>),
                ("HOME", Some(home.path().to_string_lossy().as_ref())),
            ],
            || assert!(maybe_push(&wt).is_none()),
        );
    }

    #[test]
    fn autocommit_auto_push_failure_still_commits() {
        // auto_push on (the default) but `origin` points nowhere —
        // the push fails, the mutation must not (the commit landed)
        let d = tempfile::tempdir().unwrap();
        git(d.path(), &["init", "-q"]);
        git(d.path(), &["config", "user.email", "t@t"]);
        git(d.path(), &["config", "user.name", "t"]);
        git(d.path(), &["remote", "add", "origin", "/nonexistent/bare"]);
        crate::vcs::create_orphan_branch(d.path(), "project", "init").unwrap();
        git(d.path(), &["worktree", "add", "project", "project"]);
        let wt = d.path().join("project");
        let home = tempfile::tempdir().unwrap();
        temp_env::with_vars(
            [
                ("MYCO_CONFIG", None::<&str>),
                ("XDG_CONFIG_HOME", None::<&str>),
                ("HOME", Some(home.path().to_string_lossy().as_ref())),
            ],
            || {
                std::fs::write(wt.join("TODO.md"), "# TODO\n").unwrap();
                assert!(autocommit(&wt, "chore: first", None, true).unwrap());
                assert!(maybe_push(&wt).is_some(), "push with no origin warns");
            },
        );
    }
}
