//! Git plumbing for myco — shells out to the `git` CLI (the
//! bupha-git/sturn-git convention; no git2/gix). Phase 3 builds
//! `init` / `check` / `migrate` / auto-commit on top of these
//! primitives: the `project` orphan branch + worktree, and one
//! atomic commit per verb.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Run `git -C <repo> <args>`; true on exit status 0. stdout is
/// silenced — progress chatter (`HEAD is now at …`, commit summaries)
/// must never pollute a verb's `--json` stream; stderr stays
/// inherited so failures are still diagnosable.
pub fn git(repo: &Path, args: &[&str]) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .stdout(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// A git invocation that failed, couldn't spawn, or printed non-UTF8.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Non-zero exit — stderr captured for the message.
    #[error("git {}: {}", args, stderr)]
    Git {
        /// The argv after `git -C <repo>` (for context).
        args: String,
        /// Trimmed stderr.
        stderr: String,
    },
    /// The `git` binary couldn't be spawned.
    #[error("git spawn: {0}")]
    Spawn(#[from] std::io::Error),
    /// stdout wasn't valid UTF-8.
    #[error("git output was not utf-8")]
    Utf8,
    /// A path can't be carried on git's argv — refuse to smuggle it
    /// through as an empty string.
    #[error("path is not utf-8: {}", .0.display())]
    NonUtf8Path(PathBuf),
}

/// `git -C <repo> <args>` → trimmed stdout. stdin is `/dev/null` —
/// plumbing commands that read stdin (`mktree`) get an immediate
/// EOF instead of blocking on the caller's terminal.
pub fn git_out(repo: &Path, args: &[&str]) -> Result<String, Error> {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .stdin(Stdio::null())
        .output()?;
    if !out.status.success() {
        return Err(Error::Git {
            args: args.join(" "),
            stderr: String::from_utf8_lossy(&out.stderr).trim().to_string(),
        });
    }
    String::from_utf8(out.stdout)
        .map(|s| s.trim().to_string())
        .map_err(|_| Error::Utf8)
}

/// `git -C <repo> <args>` for commands that need a committer
/// identity (`commit`, `commit-tree`). When the repo can't resolve
/// one — `git var GIT_COMMITTER_IDENT` makes the same check the
/// commit machinery does — a `-c user.name=myco
/// -c user.email=myco@localhost` fallback is injected for that one
/// invocation. A configured identity (or `GIT_*_NAME`/`GIT_*_EMAIL`
/// env) always wins, since the probe sees it first.
pub fn git_ident(repo: &Path, args: &[&str]) -> Result<String, Error> {
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
pub fn repo_root(start: &Path) -> Result<PathBuf, Error> {
    git_out(start, &["rev-parse", "--show-toplevel"]).map(PathBuf::from)
}

/// `Ok(true)` when `start` is inside a git work tree. "Not a
/// repo" (non-zero `rev-parse`) is `Ok(false)`; only spawn/output
/// failures are `Err`, so a missing `git` binary doesn't
/// masquerade as "not a repo".
pub fn is_git_repo(start: &Path) -> Result<bool, Error> {
    match git_out(start, &["rev-parse", "--is-inside-work-tree"]) {
        Ok(out) => Ok(out == "true"),
        Err(Error::Git { .. }) => Ok(false),
        Err(e) => Err(e),
    }
}

/// `refs/heads/<name>` exists.
pub fn branch_exists(repo: &Path, name: &str) -> bool {
    git(
        repo,
        &[
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/heads/{name}"),
        ],
    )
}

/// Create `name` as an orphan branch via plumbing — `mktree` an
/// empty tree, `commit-tree` a root commit, `update-ref` the branch.
/// Unlike `git checkout --orphan` this never disturbs the index or
/// working tree of the current checkout.
pub fn create_orphan_branch(repo: &Path, name: &str, msg: &str) -> Result<(), Error> {
    let tree = git_out(repo, &["mktree"])?;
    let commit = git_ident(repo, &["commit-tree", &tree, "-m", msg])?;
    // empty old-value = create-only — the ref must not exist, so a
    // race past `branch_exists` can't reset `name` to an empty
    // root and orphan the ledger
    git_out(
        repo,
        &["update-ref", &format!("refs/heads/{name}"), &commit, ""],
    )?;
    Ok(())
}

/// The branch a worktree dir has checked out (`None` when detached
/// or not a worktree).
pub fn worktree_branch(dir: &Path) -> Option<String> {
    git_out(dir, &["rev-parse", "--abbrev-ref", "HEAD"])
        .ok()
        .filter(|b| b != "HEAD" && !b.is_empty())
}

/// `git rev-parse --path-format=absolute --git-common-dir` — the
/// shared git dir for all of `dir`'s worktrees (`<main>/.git`),
/// resolved from inside any of them.
pub fn common_dir(dir: &Path) -> Result<PathBuf, Error> {
    git_out(
        dir,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )
    .map(PathBuf::from)
}

/// The main (first) worktree of the repo `dir` belongs to — i.e.
/// the repo root when `dir` is a linked worktree like `project/`.
/// Derived from `--git-common-dir` (`<main>/.git` → `<main>`).
pub fn main_worktree(dir: &Path) -> Option<PathBuf> {
    let common = common_dir(dir).ok()?;
    if common.file_name().is_some_and(|n| n == ".git") {
        common.parent().map(Path::to_path_buf)
    } else {
        // bare repo or worktree-of-worktree — no checkout root
        None
    }
}

/// Uncommitted/untracked changes under `dir` (`status --porcelain
/// -- .` non-empty). Scoped to `dir` itself — when `dir` is a plain
/// subdirectory of a bigger repo rather than a real worktree,
/// changes elsewhere in the containing repo don't count. Git
/// failures propagate: callers must not mistake "status failed"
/// for "clean".
pub fn has_changes(dir: &Path) -> Result<bool, Error> {
    git_out(dir, &["status", "--porcelain", "--", "."]).map(|s| !s.is_empty())
}

/// `git fetch origin --prune`.
pub fn sync(repo: &Path) -> bool {
    git(repo, &["fetch", "origin", "--prune"])
}

/// True when `rev` resolves to an object (`cat-file -e`).
pub fn has_commit(repo: &Path, rev: &str) -> bool {
    git(repo, &["cat-file", "-e", rev])
}

/// git argv can't carry a non-UTF-8 path — refuse rather than
/// pass an empty string that would target the wrong dir.
fn arg_path(p: &Path) -> Result<&str, Error> {
    p.to_str()
        .ok_or_else(|| Error::NonUtf8Path(p.to_path_buf()))
}

/// `git worktree add --detach <dir> <rev>` — detached scratch
/// checkout (ephemeral runs, inspections).
pub fn worktree_add_detached(repo: &Path, dir: &Path, rev: &str) -> Result<(), Error> {
    git_out(repo, &["worktree", "add", "--detach", arg_path(dir)?, rev])?;
    Ok(())
}

/// `git worktree add <dir> <branch>` — checkout `branch` into `dir`.
/// This is the `project`/`docs` worktree path: a real branch, not a
/// detached rev.
pub fn worktree_add_branch(repo: &Path, dir: &Path, branch: &str) -> Result<(), Error> {
    git_out(repo, &["worktree", "add", arg_path(dir)?, branch])?;
    Ok(())
}

/// `git worktree remove --force <dir>`.
pub fn worktree_remove(repo: &Path, dir: &Path) -> Result<(), Error> {
    git_out(repo, &["worktree", "remove", "--force", arg_path(dir)?])?;
    Ok(())
}

/// `git worktree prune` — drop metadata for deleted worktree dirs.
pub fn worktree_prune(repo: &Path) -> bool {
    git(repo, &["worktree", "prune"])
}

/// `git branch <name> <start>` — create a branch without switching.
/// Pass an empty tree commit for the `project` orphan, or use
/// `checkout --orphan` semantics via the CLI layer.
pub fn branch(repo: &Path, name: &str, start: &str) -> bool {
    git(repo, &["branch", name, start])
}

/// `git add -A -- . && git commit --allow-empty -m <msg>` —
/// the auto-commit-per-verb path writes files first, then this.
/// The stage is scoped to `repo` (`-- .`): when `repo` is a plain
/// subdirectory of a bigger repo rather than a real worktree,
/// changes elsewhere aren't swept into the ledger commit. A bare
/// `commit` could still pick up foreign *staged* changes, so those
/// are refused rather than committed under our message (a
/// `commit -- <pathspec>` alternative can't express the first
/// commit — the pathspec must match known files). `--allow-empty`
/// keeps no-op verbs honest in the audit trail.
pub fn commit_all(repo: &Path, msg: &str) -> Result<(), Error> {
    git_out(repo, &["add", "-A", "--", "."])?;
    // Foreign *staged* changes must not be swept into the ledger
    // commit — refuse them. `:(exclude).` is cwd-relative, so the
    // probe can only fire for a non-worktree dir; a real worktree
    // can't hold staged entries outside itself. The quiet diff is
    // byte-safe where a --name-only parse trips on non-UTF-8
    // filenames.
    if !git(repo, &["diff", "--cached", "--quiet", "--", ":(exclude)."]) {
        return Err(Error::Git {
            args: "commit".into(),
            stderr: "refusing to sweep staged changes outside the worktree — unstage them first"
                .into(),
        });
    }
    git_ident(repo, &["commit", "--allow-empty", "-m", msg])?;
    Ok(())
}

/// True when `refs/tags/<tag>` exists.
pub fn tag_exists(repo: &Path, tag: &str) -> bool {
    git(
        repo,
        &["rev-parse", "-q", "--verify", &format!("refs/tags/{tag}")],
    )
}

/// `git tag <tag>`.
pub fn tag(repo: &Path, tag: &str) -> bool {
    git(repo, &["tag", tag])
}

/// `git push origin <tag>`.
pub fn push_tag(repo: &Path, tag: &str) -> bool {
    git(repo, &["push", "origin", tag])
}

/// `git rev-parse --verify HEAD` — the commit a worktree/repo is
/// on, `None` when the branch is unborn. Index freshness is
/// `stored head == head_commit(worktree)`.
pub fn head_commit(dir: &Path) -> Option<String> {
    git_out(dir, &["rev-parse", "--verify", "HEAD"]).ok()
}

/// One `git log` line, parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogEntry {
    /// Full commit hash.
    pub hash: String,
    /// Author date, `YYYY-MM-DD` (`%as`).
    pub date: String,
    /// Subject line.
    pub subject: String,
}

/// `git log` newest-first — `%H %as %s` over a NUL separator so
/// subjects are safe. `limit` caps the walk (`git log -n`). An
/// unborn branch yields an empty history, not an error.
pub fn log(dir: &Path, limit: Option<usize>) -> Result<Vec<LogEntry>, Error> {
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

    fn tmpdir(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("myco-git-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap_or_default();
        d
    }

    #[test]
    fn git_runs_in_dir() {
        let d = tmpdir("version");
        assert!(git(&d, &["--version"]));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn has_commit_false_on_non_repo() {
        let d = tmpdir("nonrepo");
        assert!(!has_commit(&d, "HEAD"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn branch_commit_cycle() {
        let d = tmpdir("cycle");
        assert!(git(&d, &["init", "-q"]));
        assert!(git(&d, &["config", "user.email", "t@t"]));
        assert!(git(&d, &["config", "user.name", "t"]));
        commit_all(&d, "init").unwrap();
        assert!(has_commit(&d, "HEAD"));
        assert!(branch(&d, "project", "HEAD"));
        assert!(!tag_exists(&d, "v0"));
        assert!(tag(&d, "v0"));
        assert!(tag_exists(&d, "v0"));
        let _ = std::fs::remove_dir_all(&d);
    }

    fn repo() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        assert!(git(d.path(), &["init", "-q"]));
        assert!(git(d.path(), &["config", "user.email", "t@t"]));
        assert!(git(d.path(), &["config", "user.name", "t"]));
        d
    }

    #[test]
    fn git_out_captures_and_errors() {
        let d = repo();
        assert_eq!(
            git_out(d.path(), &["rev-parse", "--is-inside-work-tree"]).unwrap(),
            "true"
        );
        assert_eq!(repo_root(d.path()).unwrap(), d.path());
        let e = git_out(d.path(), &["rev-parse", "--verify", "nope"]).unwrap_err();
        assert!(matches!(e, Error::Git { .. }));
        assert!(e.to_string().contains("rev-parse"));
        assert!(is_git_repo(d.path()).unwrap());
        // not a repo → Ok(false), not an error
        assert!(!is_git_repo(&tmpdir("not-a-repo")).unwrap());
        assert!(repo_root(&tmpdir("not-a-repo-2")).is_err());
    }

    #[test]
    fn orphan_branch_is_unborn_root() {
        let d = repo();
        // empty repo: no HEAD yet — plumbing still works
        assert!(!branch_exists(d.path(), "project"));
        create_orphan_branch(d.path(), "project", "chore: init project ledger").unwrap();
        assert!(branch_exists(d.path(), "project"));
        // root commit exists and has no parents
        let parents = git_out(d.path(), &["rev-list", "--parents", "-n", "1", "project"])
            .unwrap()
            .split_whitespace()
            .count();
        assert_eq!(parents, 1, "root commit lists only itself");
        // main checkout untouched: still on the initial branch
        let head = git_out(d.path(), &["branch", "--show-current"]).unwrap();
        assert!(head.is_empty() || head != "project");
    }

    #[test]
    fn worktree_branch_and_main_worktree() {
        let d = repo();
        commit_all(d.path(), "c0").unwrap();
        create_orphan_branch(d.path(), "project", "init").unwrap();
        let wt = d.path().join("project");
        worktree_add_branch(d.path(), &wt, "project").unwrap();
        assert_eq!(worktree_branch(&wt).as_deref(), Some("project"));
        assert_eq!(main_worktree(&wt).unwrap(), d.path());
        // detached checkout reports no branch
        let scratch = d.path().join("scratch");
        worktree_add_detached(d.path(), &scratch, "project").unwrap();
        assert_eq!(worktree_branch(&scratch), None);
    }

    #[test]
    fn has_changes_tracks_porcelain() {
        let d = repo();
        assert!(!has_changes(d.path()).unwrap());
        std::fs::write(d.path().join("f.txt"), "x").unwrap();
        assert!(has_changes(d.path()).unwrap());
        commit_all(d.path(), "add f").unwrap();
        assert!(!has_changes(d.path()).unwrap());
    }

    #[test]
    fn has_changes_scopes_to_dir_and_propagates_errors() {
        let d = repo();
        let sub = d.path().join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        // a change elsewhere in the repo doesn't count for `sub`
        std::fs::write(d.path().join("outside.txt"), "x").unwrap();
        assert!(!has_changes(&sub).unwrap());
        std::fs::write(sub.join("inside.txt"), "x").unwrap();
        assert!(has_changes(&sub).unwrap());
        // a corrupt / non-repo dir is an error, never "clean"
        let plain = tempfile::tempdir().unwrap();
        assert!(has_changes(plain.path()).is_err());
    }

    #[test]
    fn commit_all_scopes_staging_to_dir() {
        let d = repo();
        let sub = d.path().join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(d.path().join("outside.txt"), "out").unwrap();
        std::fs::write(sub.join("inside.txt"), "in").unwrap();
        commit_all(&sub, "chore: inside").unwrap();
        let status = git_out(d.path(), &["status", "--porcelain"]).unwrap();
        assert!(status.contains("outside.txt"), "{status}");
        assert!(!status.contains("inside.txt"), "{status}");
    }

    #[test]
    fn commit_all_refuses_foreign_staged_changes() {
        let d = repo();
        let sub = d.path().join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(d.path().join("outside.txt"), "out").unwrap();
        std::fs::write(sub.join("inside.txt"), "in").unwrap();
        // a foreign change already staged must not be swept in
        git_out(d.path(), &["add", "outside.txt"]).unwrap();
        let e = commit_all(&sub, "chore: inside").unwrap_err();
        assert!(e.to_string().contains("outside the worktree"), "{e}");
        // …and the outside change is still staged, not committed
        let status = git_out(d.path(), &["status", "--porcelain"]).unwrap();
        assert!(status.contains("outside.txt"), "{status}");
    }

    #[test]
    fn commit_identity_falls_back_when_unconfigured() {
        temp_env::with_vars(
            [
                ("GIT_CONFIG_NOSYSTEM", Some("1")),
                ("GIT_CONFIG_GLOBAL", Some("/dev/null")),
                ("GIT_AUTHOR_NAME", None::<&str>),
                ("GIT_AUTHOR_EMAIL", None::<&str>),
                ("GIT_COMMITTER_NAME", None::<&str>),
                ("GIT_COMMITTER_EMAIL", None::<&str>),
            ],
            || {
                let d = tempfile::tempdir().unwrap();
                assert!(git(d.path(), &["init", "-q"]));
                // sanity: no identity is resolvable in this env
                assert!(git_out(d.path(), &["var", "GIT_COMMITTER_IDENT"]).is_err());
                std::fs::write(d.path().join("f.txt"), "x").unwrap();
                commit_all(d.path(), "chore: fresh machine").unwrap();
                let email = git_out(d.path(), &["log", "-1", "--format=%ce"]).unwrap();
                assert_eq!(email, "myco@localhost");
            },
        );
    }

    #[test]
    fn configured_identity_wins_over_fallback() {
        let d = repo(); // sets user.email t@t / user.name t
        std::fs::write(d.path().join("f.txt"), "x").unwrap();
        commit_all(d.path(), "chore: real identity").unwrap();
        let email = git_out(d.path(), &["log", "-1", "--format=%ce"]).unwrap();
        assert_eq!(email, "t@t");
    }

    #[test]
    fn head_commit_and_log() {
        let d = repo();
        // unborn HEAD → None, empty log
        assert_eq!(head_commit(d.path()), None);
        assert!(log(d.path(), None).unwrap().is_empty());
        std::fs::write(d.path().join("a.txt"), "1").unwrap();
        commit_all(d.path(), "chore: first").unwrap();
        std::fs::write(d.path().join("b.txt"), "2").unwrap();
        commit_all(d.path(), "chore: second").unwrap();
        let head = head_commit(d.path()).unwrap();
        assert_eq!(head.len(), 40);
        let entries = log(d.path(), None).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].hash, head);
        assert_eq!(entries[0].subject, "chore: second");
        assert_eq!(entries[1].subject, "chore: first");
        assert_eq!(entries[0].date.len(), 10);
        // limit caps the walk
        assert_eq!(log(d.path(), Some(1)).unwrap().len(), 1);
        // not a repo / no HEAD → empty history, not an error
        let plain = tmpdir("log-norepo");
        assert!(log(&plain, None).unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&plain);
    }

    #[test]
    fn create_orphan_branch_update_ref_failure() {
        let d = repo();
        // a stale lock file makes `update-ref` fail after the
        // commit objects were already created
        let refs = d.path().join(".git/refs/heads");
        std::fs::create_dir_all(&refs).unwrap();
        std::fs::write(refs.join("project.lock"), "").unwrap();
        let e = create_orphan_branch(d.path(), "project", "init").unwrap_err();
        assert!(matches!(e, Error::Git { .. }), "{e:?}");
    }

    #[test]
    fn create_orphan_branch_refuses_existing_branch() {
        let d = repo();
        create_orphan_branch(d.path(), "project", "init").unwrap();
        let head = git_out(d.path(), &["rev-parse", "project"]).unwrap();
        // create-only update-ref: a second attempt must fail, and
        // the branch must still point at the original root
        let e = create_orphan_branch(d.path(), "project", "clobber").unwrap_err();
        assert!(matches!(e, Error::Git { .. }), "{e:?}");
        assert_eq!(git_out(d.path(), &["rev-parse", "project"]).unwrap(), head);
    }

    #[test]
    fn worktree_fns_error_on_failure() {
        let d = repo();
        // non-utf8 paths are a clear error, not an empty argv arg
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            let bad = Path::new(std::ffi::OsStr::from_bytes(b"\xff\xfe"));
            assert!(matches!(
                worktree_add_branch(d.path(), bad, "x"),
                Err(Error::NonUtf8Path(_))
            ));
            assert!(matches!(
                worktree_add_detached(d.path(), bad, "x"),
                Err(Error::NonUtf8Path(_))
            ));
            assert!(matches!(
                worktree_remove(d.path(), bad),
                Err(Error::NonUtf8Path(_))
            ));
        }
        // ordinary failures surface stderr instead of a bare false
        let e = worktree_add_branch(d.path(), &d.path().join("w"), "no-such-branch").unwrap_err();
        assert!(matches!(e, Error::Git { .. }), "{e:?}");
        assert!(e.to_string().contains("worktree add"), "{e}");
    }

    #[test]
    fn main_worktree_none_on_bare_repo() {
        let d = tmpdir("bare");
        assert!(git(&d, &["init", "-q", "--bare", "b.git"]));
        assert_eq!(main_worktree(&d.join("b.git")), None);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn fetch_push_without_origin_fail_quietly() {
        let d = repo();
        assert!(!sync(d.path()));
        commit_all(d.path(), "c0").unwrap();
        assert!(tag(d.path(), "v1"));
        assert!(!push_tag(d.path(), "v1"));
    }

    #[test]
    fn worktree_remove_and_prune() {
        let d = repo();
        commit_all(d.path(), "c0").unwrap();
        create_orphan_branch(d.path(), "project", "init").unwrap();
        let wt = d.path().join("project");
        worktree_add_branch(d.path(), &wt, "project").unwrap();
        worktree_remove(d.path(), &wt).unwrap();
        assert!(!wt.exists());
        // remove a stale worktree dir manually, then prune metadata
        worktree_add_branch(d.path(), &wt, "project").unwrap();
        std::fs::remove_dir_all(&wt).unwrap();
        assert!(worktree_prune(d.path()));
    }

    #[test]
    fn error_display_variants() {
        let e = Error::Git {
            args: "status".into(),
            stderr: "boom".into(),
        };
        assert_eq!(e.to_string(), "git status: boom");
        assert_eq!(Error::Utf8.to_string(), "git output was not utf-8");
    }
}
