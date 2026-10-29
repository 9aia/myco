//! Git plumbing for myco — shells out to the `git` CLI (the
//! bupha-git/sturn-git convention; no git2/gix). Phase 3 builds
//! `init` / `check` / `migrate` / auto-commit on top of these
//! primitives: the `project` orphan branch + worktree, and one
//! atomic commit per verb.

use std::path::Path;
use std::process::Command;

/// Run `git -C <repo> <args>`; true on exit status 0.
pub fn git(repo: &Path, args: &[&str]) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// `git fetch origin --prune`.
pub fn sync(repo: &Path) -> bool {
    git(repo, &["fetch", "origin", "--prune"])
}

/// True when `rev` resolves to an object (`cat-file -e`).
pub fn has_commit(repo: &Path, rev: &str) -> bool {
    git(repo, &["cat-file", "-e", rev])
}

/// `git worktree add --detach <dir> <rev>` — detached scratch
/// checkout (ephemeral runs, inspections).
pub fn worktree_add_detached(repo: &Path, dir: &Path, rev: &str) -> bool {
    git(
        repo,
        &[
            "worktree",
            "add",
            "--detach",
            dir.to_str().unwrap_or_default(),
            rev,
        ],
    )
}

/// `git worktree add <dir> <branch>` — checkout `branch` into `dir`.
/// This is the `project`/`docs` worktree path: a real branch, not a
/// detached rev.
pub fn worktree_add_branch(repo: &Path, dir: &Path, branch: &str) -> bool {
    git(
        repo,
        &["worktree", "add", dir.to_str().unwrap_or_default(), branch],
    )
}

/// `git worktree remove --force <dir>`.
pub fn worktree_remove(repo: &Path, dir: &Path) -> bool {
    git(
        repo,
        &[
            "worktree",
            "remove",
            "--force",
            dir.to_str().unwrap_or_default(),
        ],
    )
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

/// `git add -A && git commit --allow-empty -m <msg>` — the
/// auto-commit-per-verb path writes files first, then this.
/// `--allow-empty` keeps no-op verbs honest in the audit trail.
pub fn commit_all(repo: &Path, msg: &str) -> bool {
    git(repo, &["add", "-A"]) && git(repo, &["commit", "--allow-empty", "-m", msg])
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
        assert!(commit_all(&d, "init"));
        assert!(has_commit(&d, "HEAD"));
        assert!(branch(&d, "project", "HEAD"));
        assert!(!tag_exists(&d, "v0"));
        assert!(tag(&d, "v0"));
        assert!(tag_exists(&d, "v0"));
        let _ = std::fs::remove_dir_all(&d);
    }
}
