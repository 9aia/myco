//! VCS access for the ledger — every git call routes through the
//! shared [`lasius_vcs`] port (P-019/P-026) backed by
//! [`lasius_vcs_git::GitVcs`], the stateless git-CLI impl.
//!
//! This module keeps the myco-shaped surface the verbs were
//! written against (free functions over a `&Path`, `io::Result`
//! errors that render as `git <args>: <stderr>`) and owns the
//! pieces that are ledger policy rather than generic VCS mechanics:
//! the `myco`/`myco@localhost` committer fallback, the
//! commit-per-verb composition ([`commit_all`]), and the
//! create-only orphan branch ([`create_orphan_branch`]).

use std::io;
use std::path::{Path, PathBuf};

use lasius_vcs::Vcs;
use lasius_vcs_git::GitVcs;

/// The backend — a unit struct, so a fresh value per call is free.
fn vcs() -> GitVcs {
    GitVcs
}

/// Re-exported so front-ends can name log entries without taking a
/// direct `lasius-vcs` dep.
pub use lasius_vcs::LogEntry;

/// `git -C <repo> <args>`; true on exit status 0. stdout is
/// silenced — progress chatter (`HEAD is now at …`, commit
/// summaries) must never pollute a verb's `--json` stream; stderr
/// is silenced too (the port's `exec`), where the retired
/// `myco-git` helper let it through for diagnosis.
pub fn git(repo: &Path, args: &[&str]) -> bool {
    vcs().exec(repo, args)
}

/// `git -C <repo> <args>` → trimmed stdout. stdin is `/dev/null` —
/// plumbing commands that read stdin (`mktree`) get an immediate
/// EOF instead of blocking on the caller's terminal. Errors render
/// `git <args>: <stderr>`, matching the retired `myco-git` error.
pub fn git_out(repo: &Path, args: &[&str]) -> io::Result<String> {
    vcs().exec_out(repo, args)
}

/// `git -C <repo> <args>` for commands that need a committer
/// identity (`commit`, `commit-tree`). When the repo can't resolve
/// one — `git var GIT_COMMITTER_IDENT` makes the same check the
/// commit machinery does — a `-c user.name=myco
/// -c user.email=myco@localhost` fallback is injected for that one
/// invocation. A configured identity (or `GIT_*_NAME`/`GIT_*_EMAIL`
/// env) always wins, since the probe sees it first.
pub fn git_ident(repo: &Path, args: &[&str]) -> io::Result<String> {
    const FALLBACK: [&str; 4] = ["-c", "user.name=myco", "-c", "user.email=myco@localhost"];
    let mut scoped = Vec::with_capacity(args.len() + FALLBACK.len());
    if git_out(repo, &["var", "GIT_COMMITTER_IDENT"]).is_err() {
        scoped.extend_from_slice(&FALLBACK);
    }
    scoped.extend_from_slice(args);
    git_out(repo, &scoped)
}

/// `git rev-parse --show-toplevel` — the worktree root containing
/// `start` (`Err` outside a repo).
pub fn repo_root(start: &Path) -> io::Result<PathBuf> {
    vcs().repo_root(start).ok_or_else(|| {
        io::Error::other(format!("{}: not inside a git repository", start.display()))
    })
}

/// `refs/heads/<name>` exists.
pub fn branch_exists(repo: &Path, name: &str) -> bool {
    vcs().branch_exists(repo, name)
}

/// Create `name` as an orphan branch via plumbing — `mktree` an
/// empty tree, `commit-tree` a root commit, `update-ref` the branch.
/// Unlike `git checkout --orphan` this never disturbs the index or
/// working tree of the current checkout.
///
/// Composed over [`git_out`]/[`git_ident`] rather than the port's
/// `create_orphan_branch` so the committer fallback stays `myco`
/// (the port's own fallback is `lasius`).
pub fn create_orphan_branch(repo: &Path, name: &str, msg: &str) -> io::Result<()> {
    let tree = git_out(repo, &["mktree"])?;
    let commit = git_ident(repo, &["commit-tree", tree.trim(), "-m", msg])?;
    // empty old-value = create-only — the ref must not exist, so a
    // race past `branch_exists` can't reset `name` to an empty
    // root and orphan the ledger
    git_out(
        repo,
        &[
            "update-ref",
            &format!("refs/heads/{name}"),
            commit.trim(),
            "",
        ],
    )?;
    Ok(())
}

/// The branch a worktree dir has checked out (`None` when detached
/// or not a worktree).
pub fn worktree_branch(dir: &Path) -> Option<String> {
    vcs().current_branch(dir)
}

/// `dir` is a git checkout whose HEAD is detached — commits made
/// there belong to no branch and become unreachable as soon as the
/// worktree moves. `None` when `dir` isn't a checkout at all
/// (the plain-directory ledger shape), which is not a defect.
pub fn is_detached(dir: &Path) -> Option<bool> {
    vcs().is_detached(dir)
}

/// Attach the checkout at `dir` to `name`, keeping its current HEAD
/// as the branch tip — the repair for a detached ledger worktree
/// (`myco check --fix`). `checkout -B` is create-or-reset, so a
/// branch left behind by an earlier run loses to the commits that
/// are actually here; git refuses when another worktree holds the
/// branch, which is precisely the case a human must arbitrate.
pub fn attach_branch(dir: &Path, name: &str) -> io::Result<()> {
    vcs().attach_branch(dir, name)
}

/// Does `origin`'s fetch refspec publish `branch` into
/// `refs/remotes/origin/`? A repo cloned with `--single-branch`
/// only tracks some branches, so `origin/<branch>` can sit stale
/// forever while `git push` still succeeds — comparing against it
/// would report "unpushed" no matter what the user does.
pub fn tracks_branch(dir: &Path, remote: &str, branch: &str) -> bool {
    vcs().tracks_branch(dir, remote, branch)
}

/// How many commits `HEAD` has that `rev` doesn't — `None` when
/// `rev` doesn't resolve (no such local or remote ref).
pub fn commits_ahead(dir: &Path, rev: &str) -> Option<usize> {
    vcs().commits_ahead(dir, rev)
}

/// How many commits `rev` has that `HEAD` doesn't — the mirror of
/// [`commits_ahead`]; `None` when `rev` doesn't resolve.
pub fn commits_behind(dir: &Path, rev: &str) -> Option<usize> {
    vcs().commits_behind(dir, rev)
}

/// `git merge --ff-only <rev>` — fast-forward `HEAD` to `rev`.
/// Fails when the histories diverged (that's `rebase`'s job).
pub fn merge_ff(dir: &Path, rev: &str) -> io::Result<()> {
    vcs().merge_ff(dir, rev)
}

/// `git rebase --autostash <rev>` — replay local commits onto
/// `rev`, stashing any dirty tree first. A conflict stops the
/// rebase mid-flight (see [`rebase_in_progress`]/[`unmerged_files`])
/// and surfaces as `Err`.
pub fn rebase(dir: &Path, rev: &str) -> io::Result<()> {
    vcs().rebase(dir, rev)
}

/// `git rebase --abort` — bail a conflicted rebase, restoring the
/// pre-rebase tip (autostash pops the worktree back too).
pub fn rebase_abort(dir: &Path) -> io::Result<()> {
    vcs().rebase_abort(dir)
}

/// A rebase/merge is stopped mid-flight in `dir` — the state dirs
/// exist (`REBASE_HEAD` alone persists after a *finished* rebase,
/// so it can't be the signal).
pub fn rebase_in_progress(dir: &Path) -> bool {
    vcs().rebase_in_progress(dir)
}

/// Worktree-relative paths with unresolved merge conflicts
/// (`status --porcelain` `XY` codes where both sides moved).
pub fn unmerged_files(dir: &Path) -> Vec<String> {
    vcs().unmerged_files(dir)
}

/// `git rev-parse --path-format=absolute --git-common-dir` — the
/// shared git dir for all of `dir`'s worktrees (`<main>/.git`),
/// resolved from inside any of them.
pub fn common_dir(dir: &Path) -> io::Result<PathBuf> {
    vcs()
        .common_dir(dir)
        .ok_or_else(|| io::Error::other(format!("{}: not inside a git repository", dir.display())))
}

/// The main (first) worktree of the repo `dir` belongs to — i.e.
/// the repo root when `dir` is a linked worktree like `project/`.
/// Derived from `--git-common-dir` (`<main>/.git` → `<main>`),
/// with a `worktree list` fallback for old git — the port's
/// `main_worktree` already folds the two together.
pub fn main_worktree(dir: &Path) -> Option<PathBuf> {
    vcs().main_worktree(dir)
}

/// Uncommitted/untracked changes under `dir` (`status --porcelain
/// -- .` non-empty). Scoped to `dir` itself — when `dir` is a plain
/// subdirectory of a bigger repo rather than a real worktree,
/// changes elsewhere in the containing repo don't count. Git
/// failures propagate: callers must not mistake "status failed"
/// for "clean".
pub fn has_changes(dir: &Path) -> io::Result<bool> {
    vcs().has_changes(dir)
}

/// `git fetch origin --prune`.
pub fn sync(repo: &Path) -> bool {
    vcs().sync(repo, Some("origin"))
}

/// `git fetch <remote> <branch>` — best-effort refresh of one
/// remote-tracking ref (no prune). `Err` on any failure —
/// callers treat fetch as advisory, never a hard gate.
pub fn fetch_branch(repo: &Path, remote: &str, branch: &str) -> io::Result<()> {
    vcs().fetch(repo, remote, &[branch])
}

/// `git push -u <remote> <branch|HEAD>` — publish `repo`'s
/// checked-out branch (`None` = HEAD). `Err` carries git's
/// stderr; callers treat push as advisory — the commit it
/// follows already landed locally.
pub fn push_branch(repo: &Path, remote: &str, branch: Option<&str>) -> io::Result<()> {
    vcs().push_branch(repo, remote, branch)
}

/// True when `rev` resolves to an object (`cat-file -e`).
pub fn has_commit(repo: &Path, rev: &str) -> bool {
    vcs().has_commit(repo, rev)
}

/// Checkout `branch` into `dir` — the `project`/`docs` worktree
/// path: a real branch, not a detached rev. The branch must
/// already exist (orphan-created or adopted from the remote);
/// the port's `worktree_add_existing`, not `worktree_add_branch`
/// which would create a new branch at the rev.
pub fn worktree_add_branch(repo: &Path, dir: &Path, branch: &str) -> io::Result<()> {
    vcs().worktree_add_existing(repo, dir, branch)
}

/// `git add -A -- . && git commit --allow-empty -m <msg>` —
/// the auto-commit-per-verb path writes files first, then this.
/// The stage is scoped to `repo` (`-- .`): when `repo` is a plain
/// subdirectory of a bigger repo rather than a real worktree,
/// changes elsewhere aren't swept into the ledger commit (the
/// port's `stage_all` is a whole-tree `add -A` — too broad here).
/// A bare `commit` could still pick up foreign *staged* changes,
/// so those are refused rather than committed under our message
/// (a `commit -- <pathspec>` alternative can't express the first
/// commit — the pathspec must match known files). `--allow-empty`
/// keeps no-op verbs honest in the audit trail.
pub fn commit_all(repo: &Path, msg: &str) -> io::Result<()> {
    // A detached checkout has no branch to commit onto: the write
    // would succeed and the audit trail would quietly vanish. Refuse
    // before touching the index (`check --fix` reattaches, and the
    // repair paths — `commit`, `migrate`, `check` — take the lock
    // directly, so they still reach a detached ledger).
    if is_detached(repo) == Some(true) {
        return Err(io::Error::other(format!(
            "{} is on a detached HEAD — a commit would be unreachable; run `myco check --fix`",
            repo.display()
        )));
    }
    git_out(repo, &["add", "-A", "--", "."])?;
    // Foreign *staged* changes must not be swept into the ledger
    // commit — refuse them. `:(exclude).` is cwd-relative, so the
    // probe can only fire for a non-worktree dir; a real worktree
    // can't hold staged entries outside itself. The quiet diff is
    // byte-safe where a --name-only parse trips on non-UTF-8
    // filenames.
    if !git(repo, &["diff", "--cached", "--quiet", "--", ":(exclude)."]) {
        return Err(io::Error::other(
            "git commit: refusing to sweep staged changes outside the worktree — unstage them first",
        ));
    }
    git_ident(repo, &["commit", "--allow-empty", "-m", msg])?;
    Ok(())
}

/// `git rev-parse --verify HEAD` — the commit a worktree/repo is
/// on, `None` when the branch is unborn. Index freshness is
/// `stored head == head_commit(worktree)`.
pub fn head_commit(dir: &Path) -> Option<String> {
    vcs().head(dir)
}

/// One `git log` line, parsed — re-exported [`LogEntry`].
///
/// `git log` newest-first — `%H %as %s` over a NUL separator so
/// subjects are safe. `limit` caps the walk (`git log -n`); `None`
/// walks the whole history. An unborn branch yields an empty
/// history, not an error.
pub fn log(dir: &Path, limit: Option<usize>) -> io::Result<Vec<LogEntry>> {
    if !has_commit(dir, "HEAD") {
        return Ok(Vec::new());
    }
    let mut args = vec!["log", "--format=%H%x00%as%x00%s"];
    let n;
    if let Some(k) = limit {
        n = format!("-{k}");
        args.push(&n);
    }
    let out = git_out(dir, &args)?;
    Ok(out
        .lines()
        .filter_map(|line| {
            let mut parts = line.split('\0');
            Some(LogEntry {
                hash: parts.next()?.to_string(),
                date: parts.next()?.to_string(),
                subject: parts.next().unwrap_or_default().to_string(),
            })
        })
        .collect())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn repo() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        assert!(git(d.path(), &["init", "-q"]));
        assert!(git(d.path(), &["config", "user.email", "t@t"]));
        assert!(git(d.path(), &["config", "user.name", "t"]));
        d
    }

    #[test]
    fn orphan_branch_uses_myco_ident() {
        let d = repo();
        // Isolate every identity source — local (unset below),
        // global/system config, XDG, HOME, and GIT_*_NAME/EMAIL env
        // — so `git var GIT_COMMITTER_IDENT` fails and the `myco`
        // fallback is what commit-tree sees. Nextest runs each test
        // in its own process, so env mutation can't race.
        let home = tempfile::tempdir().unwrap();
        temp_env::with_vars(
            [
                ("GIT_CONFIG_NOSYSTEM", Some("1")),
                ("GIT_CONFIG_GLOBAL", Some("/dev/null")),
                ("GIT_CONFIG_SYSTEM", Some("/dev/null")),
                ("HOME", Some(home.path().to_str().unwrap())),
                ("XDG_CONFIG_HOME", Some(home.path().to_str().unwrap())),
                ("GIT_AUTHOR_NAME", None::<&str>),
                ("GIT_AUTHOR_EMAIL", None::<&str>),
                ("GIT_COMMITTER_NAME", None::<&str>),
                ("GIT_COMMITTER_EMAIL", None::<&str>),
            ],
            || {
                git(d.path(), &["config", "--unset", "user.email"]);
                git(d.path(), &["config", "--unset", "user.name"]);
                assert!(!branch_exists(d.path(), "project"));
                create_orphan_branch(d.path(), "project", "chore: init project ledger").unwrap();
                assert!(branch_exists(d.path(), "project"));
                let author =
                    git_out(d.path(), &["log", "-1", "--format=%an <%ae>", "project"]).unwrap();
                assert_eq!(author, "myco <myco@localhost>");
                // create-only: a second call must fail, not reset the ref
                assert!(create_orphan_branch(d.path(), "project", "again").is_err());
            },
        );
    }

    #[test]
    fn commit_all_scopes_and_commits() {
        let d = repo();
        std::fs::write(d.path().join("a.txt"), "a").unwrap();
        commit_all(d.path(), "chore: test").unwrap();
        assert!(has_commit(d.path(), "HEAD"));
        // clean tree → --allow-empty still commits
        commit_all(d.path(), "chore: noop").unwrap();
        let n = git_out(d.path(), &["rev-list", "--count", "HEAD"]).unwrap();
        assert_eq!(n.trim(), "2");
        // detached HEAD refuses before touching the index
        let sha = git_out(d.path(), &["rev-parse", "HEAD"]).unwrap();
        git(d.path(), &["checkout", "--detach", "-q", sha.trim()]);
        assert_eq!(is_detached(d.path()), Some(true));
        std::fs::write(d.path().join("b.txt"), "b").unwrap();
        let e = commit_all(d.path(), "chore: nope").unwrap_err();
        assert!(e.to_string().contains("detached HEAD"), "{e}");
    }

    #[test]
    fn worktree_add_branch_checks_out_existing() {
        let d = repo();
        commit_all(d.path(), "chore: seed").unwrap();
        assert!(!branch_exists(d.path(), "project"));
        create_orphan_branch(d.path(), "project", "chore: init project ledger").unwrap();
        let wt = d.path().join("wt");
        worktree_add_branch(d.path(), &wt, "project").unwrap();
        assert_eq!(worktree_branch(&wt).as_deref(), Some("project"));
        // the branch was NOT recreated — its tip is still the orphan root
        let tip = git_out(d.path(), &["rev-parse", "project"]).unwrap();
        assert_ne!(
            tip.trim(),
            git_out(d.path(), &["rev-parse", "HEAD"]).unwrap().trim()
        );
        // a second add of the same branch fails (already checked out)
        let wt2 = d.path().join("wt2");
        assert!(worktree_add_branch(d.path(), &wt2, "project").is_err());
    }
}
