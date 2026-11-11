//! Project discovery — resolve the `project/` worktree a verb
//! should act on.
//!
//! Order: `--dir` (explicit worktree) → `--root` (repo root →
//! `<root>/project`) → cwd inside the worktree itself →
//! `myco.toml`'s `project_dir` (only the discovered repo's own
//! `myco.toml`, and only a path that stays inside it) → walk up
//! from cwd for a `project/` dir that is a git worktree on branch
//! `project` (or, weaker, any `project/` holding a `TODO.md`).
//! The walk is capped at the worktree root containing cwd — an
//! ancestor's `project/` above it can be a *different* repo's
//! ledger (git worktrees live anywhere) and must never win.

use std::path::{Component, Path, PathBuf};

use crate::config::ConfigLoader;

/// The orphan branch + conventional dir name.
pub const BRANCH: &str = "project";
/// Conventional worktree dir at the repo root.
pub const DIR: &str = "project";

/// A resolved project: repo root + the `project/` worktree inside
/// (or pointed at via `--dir`).
#[derive(Debug)]
pub struct Project {
    /// Main worktree — the repo the ledger belongs to.
    pub root: PathBuf,
    /// The project worktree dir (`<root>/project` by convention).
    pub worktree: PathBuf,
}

/// Canonical form for comparisons — never fails, degrades to the
/// input when `canonicalize` can't resolve the path.
fn canon(p: &Path) -> PathBuf {
    p.canonicalize().unwrap_or_else(|_| p.to_path_buf())
}

/// `d` is the ROOT of a git worktree — `rev-parse` run in a
/// subdirectory reports the enclosing worktree's root and branch,
/// so `project/sub` would otherwise pass the branch check and be
/// mistaken for the ledger.
fn is_worktree_root(d: &Path) -> bool {
    myco_git::repo_root(d)
        .map(|top| canon(&top) == canon(d))
        .unwrap_or(false)
}

/// `d` is usable as the project dir — a git worktree rooted at
/// `d` checked out on `project`, or at minimum a dir holding
/// `TODO.md`.
pub fn qualifies(d: &Path) -> bool {
    if !d.is_dir() {
        return false;
    }
    (is_worktree_root(d) && myco_git::worktree_branch(d).as_deref() == Some(BRANCH))
        || d.join("TODO.md").is_file()
}

/// The worktree root enclosing `cwd` is itself on `project` —
/// handles running myco from inside `project/` (any depth).
/// `rev-parse --show-toplevel` finds the real worktree root, so a
/// nested dir never gets mistaken for the ledger.
fn enclosing_project_worktree(cwd: &Path) -> Option<PathBuf> {
    let top = myco_git::repo_root(cwd).ok()?;
    (myco_git::worktree_branch(&top).as_deref() == Some(BRANCH)).then_some(top)
}

/// `candidate` — a qualifying `project/` dir found under `a` —
/// actually belongs to the repo containing `a`. A git worktree
/// can be mounted anywhere, so `<a>/project` may be a different
/// repo's ledger parked in this tree; attaching it would make
/// verbs stage, scan, and commit against the wrong root.
fn owns_project(a: &Path, candidate: &Path) -> bool {
    // only a worktree root has a repo of its own — a plain dir
    // (TODO.md-qualified) or a subdirectory of `a`'s tree
    // physically lives in `a`
    if !is_worktree_root(candidate) {
        return true;
    }
    match (myco_git::common_dir(a), myco_git::common_dir(candidate)) {
        // same shared git dir → same repo (covers `a` being a
        // linked worktree itself)
        (Ok(x), Ok(y)) => canon(&x) == canon(&y),
        // `a` isn't inside a repo — accept only when `candidate`'s
        // main worktree IS `a` (the ledger's repo rooted there);
        // a worktree whose main worktree lives elsewhere is
        // foreign
        _ => myco_git::main_worktree_compat(candidate).is_some_and(|m| canon(&m) == canon(a)),
    }
}

/// `project_dir` for the repo rooted at `root` — must be a
/// non-empty, relative path that can't escape the repo (absolute
/// paths and `..` components would let the config point at a
/// foreign dir). Sources: an explicit `MYCO_CONFIG` path (the
/// user pointed at it) or the repo's own `<root>/myco.toml`. A
/// `myco.toml` found relative to an arbitrary cwd — or the
/// `~/.config` fallback, which would apply one `project_dir` to
/// *every* repo — must not make repo B adopt repo A's ledger.
/// The path policy is ours; `cfg` supplies the file format.
fn configured_project_dir(root: &Path, cfg: &ConfigLoader) -> Option<PathBuf> {
    let path = std::env::var("MYCO_CONFIG")
        .map(PathBuf::from)
        .unwrap_or_else(|_| root.join("myco.toml"));
    let conf = cfg.load(Some(&path)).ok()?;
    let rel = PathBuf::from(conf.project_dir?);
    let safe = !rel.as_os_str().is_empty()
        && rel
            .components()
            .all(|c| matches!(c, Component::Normal(_) | Component::CurDir));
    safe.then_some(rel)
}

fn canonicalize(p: &Path) -> Result<PathBuf, String> {
    p.canonicalize()
        .map_err(|e| format!("{}: {e}", p.display()))
}

/// Canonical `--dir` even when the target doesn't exist yet —
/// canonicalize the existing parent and re-attach the basename.
fn canonical_dir(d: &Path) -> PathBuf {
    if let Ok(p) = d.canonicalize() {
        return p;
    }
    let Some(name) = d.file_name() else {
        return d.to_path_buf();
    };
    // `foo`'s parent is the empty path — that means cwd
    let parent = d
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    parent
        .canonicalize()
        .map(|p| p.join(name))
        .unwrap_or_else(|_| d.to_path_buf())
}

/// Resolve the project for verbs that need one. `dir`/`root` are
/// the global flags; `cfg` loads `myco.toml` (`project_dir`) via
/// the front-end's impl; everything else is discovery.
pub fn discover(
    dir: Option<&Path>,
    root: Option<&Path>,
    cfg: ConfigLoader,
) -> Result<Project, String> {
    discover_with(dir, root, cfg, None)
}

/// [`discover`] with the repo index available — the last-resort
/// fallback for the org-ledger pattern: when plain discovery
/// fails inside a *registered* repo that has no ledger of its
/// own, the org entry's worktree answers instead. `None` (or an
/// unreadable index) keeps discovery exactly as strict as before
/// — unregistered repos still fail.
pub fn discover_with(
    dir: Option<&Path>,
    root: Option<&Path>,
    cfg: ConfigLoader,
    index: Option<&dyn myco_index::Index>,
) -> Result<Project, String> {
    let cwd = std::env::current_dir().map_err(|e| format!("cwd: {e}"))?;
    discover_in_index(&cwd, dir, root, cfg, index)
}

/// [`discover`] with an explicit cwd — the workhorse, and what
/// tests and the index universe drive (no process-global
/// `set_current_dir` races). No index: the org-ledger fallback
/// stays off.
pub fn discover_at(
    cwd: &Path,
    dir: Option<&Path>,
    root: Option<&Path>,
    cfg: ConfigLoader,
) -> Result<Project, String> {
    discover_in_index(cwd, dir, root, cfg, None)
}

fn discover_in_index(
    cwd: &Path,
    dir: Option<&Path>,
    root: Option<&Path>,
    cfg: ConfigLoader,
    index: Option<&dyn myco_index::Index>,
) -> Result<Project, String> {
    // --dir: an explicit worktree path — still must qualify,
    // otherwise verbs would stage/commit inside whatever repo
    // happens to contain an arbitrary dir
    if let Some(d) = dir {
        let worktree = canonicalize(d)?;
        if !worktree.is_dir() {
            return Err(format!("--dir {} is not a directory", worktree.display()));
        }
        if !qualifies(&worktree) {
            return Err(format!(
                "--dir {} is not a myco project worktree — \
                 run `myco init --dir {}` inside its repo, \
                 or pass --root for the repo's conventional project/",
                worktree.display(),
                d.display()
            ));
        }
        let root = match root {
            Some(r) => canonicalize(r)?,
            None => myco_git::main_worktree_compat(&worktree)
                .or_else(|| worktree.parent().map(Path::to_path_buf))
                .unwrap_or_else(|| worktree.clone()),
        };
        return Ok(Project { root, worktree });
    }

    // --root: conventional <root>/project — `--root` names the
    // repo, not its ledger, so the org fallback still applies
    if let Some(r) = root {
        let root = canonicalize(r)?;
        let worktree = root.join(DIR);
        if qualifies(&worktree) {
            return Ok(Project { root, worktree });
        }
        if let Some(p) = index.and_then(|i| org_fallback(&root, i, cfg)) {
            return Ok(p);
        }
        return Err(format!(
            "{} is not a myco project worktree — run `myco init`",
            worktree.display()
        ));
    }

    let cwd = canon(cwd);

    // cwd inside the project worktree itself — checked before any
    // `project/` found by config or walking: an ancestor's ledger
    // may be a different repo's
    if let Some(wt) = enclosing_project_worktree(&cwd) {
        let root = myco_git::main_worktree_compat(&wt)
            .or_else(|| wt.parent().map(Path::to_path_buf))
            .unwrap_or_else(|| wt.clone());
        return Ok(Project { root, worktree: wt });
    }

    // myco.toml's `project_dir` — read from the discovered repo's
    // own config, inside the repo only
    if let Ok(root) = myco_git::repo_root(&cwd) {
        if let Some(rel) = configured_project_dir(&root, &cfg) {
            let worktree = root.join(rel);
            if qualifies(&worktree) && owns_project(&root, &worktree) {
                return Ok(Project { root, worktree });
            }
        }
    }

    // walk up: `<ancestor>/project` that qualifies *and belongs
    // to the same repo* — capped at the worktree root containing
    // cwd so a higher ancestor's foreign `project/` can't win
    let top = myco_git::repo_root(&cwd).ok().map(|t| canon(&t));
    for a in cwd.ancestors() {
        if top.as_ref().is_some_and(|t| !a.starts_with(t)) {
            break;
        }
        let candidate = a.join(DIR);
        if candidate != *a && qualifies(&candidate) && owns_project(a, &candidate) {
            return Ok(Project {
                root: a.to_path_buf(),
                worktree: candidate,
            });
        }
    }

    // org-ledger fallback — only repos in the index universe
    // (lasius clones ∪ registry) get here; others still fail
    if let Some(index) = index {
        if let Ok(top) = myco_git::repo_root(&cwd) {
            if let Some(p) = org_fallback(&top, index, cfg) {
                return Ok(p);
            }
        }
    }

    Err(no_project_error(&cwd))
}

/// The index-backed fallback — `repo` is the git toplevel cwd
/// (or `--root`) resolved to, and doubles as the lasius-workspace
/// anchor: inside a workspace the universe is its clones (P-026),
/// outside it the `repos.toml` registry. Two hits, in order:
///
/// 1. the repo's own ledger when it qualifies — covers custom
///    `worktree`s the cwd walk can't see;
/// 2. the `ledger = "org"` entry's worktree — the org/global
///    ledger ledger-less members resolve to (`myco.toml`
///    `[index] ledger` or a legacy `repos.toml` mark).
///
/// `None` when the repo isn't in the universe, has no qualifying
/// ledger anywhere, or no org ledger is designated/valid — the
/// caller then reports the ordinary "no project" error.
fn org_fallback(repo: &Path, index: &dyn myco_index::Index, cfg: ConfigLoader) -> Option<Project> {
    let repos = crate::universe::repos(repo, index, cfg)
        .ok()?
        .into_iter()
        .map(|u| u.entry)
        .collect::<Vec<_>>();
    // registry keys are canonical repo roots — match the arg and
    // its main worktree (cwd may sit in a linked worktree whose
    // toplevel isn't the registered root)
    let mut keys = vec![canon(repo)];
    if let Some(m) = myco_git::main_worktree_compat(repo) {
        let m = canon(&m);
        if !keys.contains(&m) {
            keys.push(m);
        }
    }
    let entry = keys.iter().find_map(|k| {
        repos
            .iter()
            .find(|r| canon(Path::new(r.path.as_str())) == *k)
    })?;
    if let Some(wt) = entry.worktree_path() {
        let wt = wt.as_std_path();
        if qualifies(wt) {
            return Some(Project {
                root: entry.path.as_std_path().to_path_buf(),
                worktree: canon(wt),
            });
        }
    }
    let org = repos
        .iter()
        .find(|r| r.ledger == myco_index::LedgerMode::Org)?;
    let wt = org.worktree_path()?;
    let wt = wt.as_std_path();
    if !qualifies(wt) {
        return None;
    }
    Some(Project {
        root: org.path.as_std_path().to_path_buf(),
        worktree: canon(wt),
    })
}

/// Tailor the dead-end message — `init` can only help inside a repo.
fn no_project_error(cwd: &Path) -> String {
    match myco_git::repo_root(cwd) {
        Ok(root) => format!(
            "no project worktree found above {} — run `myco init` in {}",
            cwd.display(),
            root.display()
        ),
        Err(_) => format!(
            "no myco project above {} — not inside a git repository",
            cwd.display()
        ),
    }
}

/// Resolve the repo root for `myco init` / `migrate` — the verbs
/// that create the worktree rather than requiring it. `--root`
/// wins; then `--dir` — the ledger belongs to the repo whose work
/// tree contains the target, and a linked-worktree container
/// resolves to the repo's *main* worktree so `.gitignore`/
/// `AGENTS.md` land on the main checkout, not a worktree's
/// feature branch; then the git repo containing cwd.
pub fn repo_root_for(dir: Option<&Path>, root: Option<&Path>) -> Result<PathBuf, String> {
    if let Some(r) = root {
        return canonicalize(r);
    }
    if let Some(d) = dir {
        // `--dir` needn't exist yet — the owning repo is found via
        // its (canonicalized) parent
        let d = canonical_dir(d);
        let parent = d.parent().unwrap_or(d.as_path());
        let container = myco_git::repo_root(parent)
            .map_err(|_| format!("{}: not inside a git repository", parent.display()))?;
        return Ok(myco_git::main_worktree_compat(&container).unwrap_or(container));
    }
    let cwd = std::env::current_dir().map_err(|e| format!("cwd: {e}"))?;
    myco_git::repo_root(&cwd).map_err(|_| {
        format!(
            "myco init: {} is not inside a git repository",
            cwd.display()
        )
    })
}

/// The worktree path `init`/`migrate` should use — `--dir`
/// (canonicalized) or the conventional `<root>/project`.
pub fn worktree_path(dir: Option<&Path>, root: &Path) -> PathBuf {
    dir.map(canonical_dir).unwrap_or_else(|| root.join(DIR))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::config::test_loader;
    use std::process::Command;

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

    /// A repo with an initialized project worktree.
    fn repo_with_project() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        let r = d.path();
        git(r, &["init", "-q"]);
        git(r, &["config", "user.email", "t@t"]);
        git(r, &["config", "user.name", "t"]);
        myco_git::create_orphan_branch(r, "project", "init").unwrap();
        git(r, &["worktree", "add", "project", "project"]);
        std::fs::write(r.join("project/TODO.md"), "# TODO — x\n").unwrap();
        d
    }

    #[test]
    fn dir_flag_resolves() {
        let d = repo_with_project();
        let p = discover(Some(&d.path().join("project")), None, test_loader()).unwrap();
        assert_eq!(p.worktree, d.path().join("project").canonicalize().unwrap());
        assert_eq!(p.root, d.path());
    }

    #[test]
    fn root_flag_resolves_and_errors() {
        let d = repo_with_project();
        let p = discover(None, Some(d.path()), test_loader()).unwrap();
        assert_eq!(p.worktree, d.path().join("project"));
        // root without a qualifying project/ → error
        let empty = tempfile::tempdir().unwrap();
        assert!(discover(None, Some(empty.path()), test_loader()).is_err());
    }

    #[test]
    fn dir_flag_missing_is_error() {
        assert!(discover(Some(Path::new("/definitely/not/here")), None, test_loader()).is_err());
    }

    #[test]
    fn dir_flag_rejects_non_project_dir() {
        // an arbitrary dir isn't a project worktree just because
        // it exists — the error must name the fix
        let d = tempfile::tempdir().unwrap();
        let e = discover(Some(d.path()), None, test_loader()).unwrap_err();
        assert!(e.contains("myco init"), "{e}");
        // a dir holding ledger files still qualifies
        std::fs::write(d.path().join("TODO.md"), "# TODO\n").unwrap();
        assert!(discover(Some(d.path()), None, test_loader()).is_ok());
    }

    #[test]
    fn qualifies_needs_dir() {
        assert!(!qualifies(Path::new("/nonexistent")));
        let d = tempfile::tempdir().unwrap();
        // plain dir without TODO.md or project branch → no
        assert!(!qualifies(d.path()));
        std::fs::write(d.path().join("TODO.md"), "x").unwrap();
        assert!(qualifies(d.path()));
    }

    #[test]
    fn worktree_subdir_is_not_the_ledger() {
        // `rev-parse` from `project/sub` reports the enclosing
        // worktree's branch — the subdir must not qualify, and
        // discovery from inside it must land on the real worktree
        let d = repo_with_project();
        let sub = d.path().join("project/sub");
        std::fs::create_dir(&sub).unwrap();
        assert!(!qualifies(&sub));
        temp_env::with_var("MYCO_CONFIG", None::<&str>, || {
            let p = discover_at(&sub, None, None, test_loader()).unwrap();
            assert_eq!(p.worktree, d.path().join("project").canonicalize().unwrap());
            assert_eq!(p.root, d.path());
        });
    }

    #[test]
    fn walk_does_not_attach_foreign_repo_project() {
        // repo A has its ledger at `a/project`; repo B is a nested
        // repo inside a's tree — running from B must not attach
        // A's ledger (the walk is capped at B's toplevel)
        let a = repo_with_project();
        let b = a.path().join("b");
        std::fs::create_dir(&b).unwrap();
        git(&b, &["init", "-q"]);
        temp_env::with_var("MYCO_CONFIG", None::<&str>, || {
            assert!(
                discover_at(&b, None, None, test_loader()).is_err(),
                "nested repo attached the outer repo's project/"
            );
            // …while a's own tree still resolves as before
            let p = discover_at(a.path(), None, None, test_loader()).unwrap();
            assert_eq!(p.worktree, a.path().join("project").canonicalize().unwrap());
            assert_eq!(p.root, a.path());
        });
    }

    #[test]
    fn walk_rejects_foreign_worktree_mounted_inside_repo() {
        // repo C's worktree mounted *inside* repo B's tree at the
        // conventional path — it qualifies (TODO.md) but belongs
        // to C, not B
        let c = repo_with_project();
        let b = tempfile::tempdir().unwrap();
        git(b.path(), &["init", "-q"]);
        let mounted = b.path().join("project");
        git(
            c.path(),
            &[
                "worktree",
                "add",
                "--detach",
                mounted.to_str().unwrap(),
                "project",
            ],
        );
        std::fs::write(mounted.join("TODO.md"), "# TODO — c\n").unwrap();
        temp_env::with_var("MYCO_CONFIG", None::<&str>, || {
            assert!(
                discover_at(b.path(), None, None, test_loader()).is_err(),
                "attached a foreign repo's worktree"
            );
            // …and B's *own* ledger resolves fine
            myco_git::create_orphan_branch(b.path(), "project", "init").unwrap();
            let own = b.path().join("own-ledger");
            git(
                b.path(),
                &["worktree", "add", own.to_str().unwrap(), "project"],
            );
            let p = discover_at(&own, None, None, test_loader()).unwrap();
            assert_eq!(p.worktree, own.canonicalize().unwrap());
            assert_eq!(p.root, b.path());
        });
    }

    #[test]
    fn project_dir_config_must_be_relative_and_repo_owned() {
        temp_env::with_var("MYCO_CONFIG", None::<&str>, || {
            // a non-conventional ledger dir named by the repo's own
            // myco.toml resolves — from any depth inside the repo
            let d = tempfile::tempdir().unwrap();
            let r = d.path();
            git(r, &["init", "-q"]);
            git(r, &["config", "user.email", "t@t"]);
            git(r, &["config", "user.name", "t"]);
            myco_git::create_orphan_branch(r, "project", "init").unwrap();
            git(r, &["worktree", "add", "pm", "project"]);
            std::fs::write(r.join("myco.toml"), "project_dir = \"pm\"\n").unwrap();
            let sub = r.join("sub");
            std::fs::create_dir(&sub).unwrap();
            let p = discover_at(&sub, None, None, test_loader()).unwrap();
            assert_eq!(p.worktree, r.join("pm"));

            // `..` and absolute project_dir values are ignored —
            // they would point discovery outside the repo
            for bad in ["project_dir = \"../pm\"", "project_dir = \"/tmp/pm\""] {
                std::fs::write(r.join("myco.toml"), format!("{bad}\n")).unwrap();
                assert!(
                    discover_at(&sub, None, None, test_loader()).is_err(),
                    "{bad}"
                );
            }
        });
    }

    #[test]
    fn no_project_error_adapts_to_git_context() {
        // inside a repo → init hint naming the root
        let d = tempfile::tempdir().unwrap();
        git(d.path(), &["init", "-q"]);
        let msg = no_project_error(d.path());
        assert!(msg.contains("run `myco init` in"), "{msg}");

        // outside any repo → no init hint
        let outside = tempfile::tempdir().unwrap();
        let msg = no_project_error(outside.path());
        assert!(msg.contains("not inside a git repository"), "{msg}");
    }

    #[test]
    fn repo_root_for_variants() {
        let d = repo_with_project();
        assert_eq!(repo_root_for(None, Some(d.path())).unwrap(), d.path());
        assert_eq!(
            repo_root_for(Some(&d.path().join("project")), None).unwrap(),
            d.path().canonicalize().unwrap()
        );
        assert!(repo_root_for(Some(Path::new("/not/here")), None).is_err());
    }

    #[test]
    fn repo_root_for_dir_resolves_to_main_worktree() {
        // --dir inside a *linked* worktree's tree → root files
        // land on the repo's main worktree, not the worktree's
        // feature branch
        let d = repo_with_project();
        let linked = tempfile::tempdir().unwrap();
        let linked = linked.path().join("feat");
        // `master` is unborn (only the `project` orphan exists) —
        // the linked worktree just needs a branch of the same repo
        git(d.path(), &["branch", "feat", "project"]);
        git(
            d.path(),
            &["worktree", "add", linked.to_str().unwrap(), "feat"],
        );
        let target = linked.join("ledger");
        assert_eq!(
            repo_root_for(Some(&target), None).unwrap(),
            d.path().canonicalize().unwrap()
        );
        // and the worktree path comes back canonical even though
        // `ledger` doesn't exist yet
        assert_eq!(
            worktree_path(Some(&target), d.path()),
            linked.canonicalize().unwrap().join("ledger")
        );
    }

    #[test]
    fn worktree_path_prefers_dir() {
        let d = tempfile::tempdir().unwrap();
        let explicit = Path::new("/tmp/custom");
        assert_eq!(
            worktree_path(Some(explicit), d.path()),
            Path::new("/tmp").canonicalize().unwrap().join("custom")
        );
        assert_eq!(worktree_path(None, d.path()), d.path().join("project"));
    }

    // --- org-ledger fallback ------------------------------------------------

    use myco_index::{Index, LedgerMode, RepoEntry};
    use myco_index_redb::RedbIndex;

    /// A plain git repo with no `project/` — the ledger-less
    /// member shape.
    fn repo_without_project() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        git(d.path(), &["init", "-q"]);
        git(d.path(), &["config", "user.email", "t@t"]);
        git(d.path(), &["config", "user.name", "t"]);
        d
    }

    fn canon_path(p: &Path) -> camino::Utf8PathBuf {
        camino::Utf8PathBuf::from_path_buf(p.canonicalize().unwrap()).unwrap()
    }

    /// The org pattern: one `ledger = "org"` repo holding the
    /// ledger, member repos registered `none`.
    #[test]
    fn org_fallback_resolves_ledgerless_member() {
        temp_env::with_var("MYCO_CONFIG", None::<&str>, || {
            let org = repo_with_project();
            let member = repo_without_project();
            let stray = repo_without_project(); // registered nowhere
            let idx_dir = tempfile::tempdir().unwrap();
            let index = RedbIndex::open_at(
                camino::Utf8PathBuf::from_path_buf(idx_dir.path().join("idx")).unwrap(),
            )
            .unwrap();
            index
                .register(
                    &RepoEntry::new(canon_path(org.path()), "org")
                        .with_worktree(canon_path(&org.path().join("project")))
                        .with_ledger(LedgerMode::Org),
                )
                .unwrap();
            index
                .register(
                    &RepoEntry::new(canon_path(member.path()), "member")
                        .with_ledger(LedgerMode::None),
                )
                .unwrap();

            // inside the member → the org ledger's worktree
            let p =
                discover_in_index(member.path(), None, None, test_loader(), Some(&index)).unwrap();
            assert_eq!(
                p.worktree,
                org.path().join("project").canonicalize().unwrap()
            );
            assert_eq!(p.root, org.path().canonicalize().unwrap());

            // --root naming the member resolves the same way
            let p = discover_in_index(
                member.path(),
                None,
                Some(member.path()),
                test_loader(),
                Some(&index),
            )
            .unwrap();
            assert_eq!(
                p.worktree,
                org.path().join("project").canonicalize().unwrap()
            );

            // the unregistered repo still fails — registration is
            // the opt-in
            assert!(
                discover_in_index(stray.path(), None, None, test_loader(), Some(&index)).is_err()
            );
            // …and so does the member without an index
            assert!(discover_in_index(member.path(), None, None, test_loader(), None).is_err());
        });
    }

    #[test]
    fn explicit_overrides_beat_org_fallback() {
        temp_env::with_var("MYCO_CONFIG", None::<&str>, || {
            let org = repo_with_project();
            let member = repo_without_project();
            let other = repo_with_project();
            let idx_dir = tempfile::tempdir().unwrap();
            let index = RedbIndex::open_at(
                camino::Utf8PathBuf::from_path_buf(idx_dir.path().join("idx")).unwrap(),
            )
            .unwrap();
            index
                .register(
                    &RepoEntry::new(canon_path(org.path()), "org")
                        .with_worktree(canon_path(&org.path().join("project")))
                        .with_ledger(LedgerMode::Org),
                )
                .unwrap();
            index
                .register(
                    &RepoEntry::new(canon_path(member.path()), "member")
                        .with_ledger(LedgerMode::None),
                )
                .unwrap();

            // `--dir` names the worktree outright — the org ledger
            // must not answer instead
            let p = discover_in_index(
                member.path(),
                Some(&other.path().join("project")),
                None,
                test_loader(),
                Some(&index),
            )
            .unwrap();
            assert_eq!(p.root, other.path().canonicalize().unwrap());
            assert_eq!(
                p.worktree,
                other.path().join("project").canonicalize().unwrap()
            );

            // `--root` names the repo — its own qualifying
            // `project/` wins over the member's org fallback
            let p = discover_in_index(
                member.path(),
                None,
                Some(other.path()),
                test_loader(),
                Some(&index),
            )
            .unwrap();
            assert_eq!(p.root, other.path().canonicalize().unwrap());

            // `project_dir` in the member's own myco.toml is an
            // override too — a qualifying repo-local ledger dir
            // beats the org fallback even though the registry says
            // `none`
            let ledger = member.path().join("ledger");
            std::fs::create_dir(&ledger).unwrap();
            std::fs::write(ledger.join("TODO.md"), "# TODO\n").unwrap();
            std::fs::write(
                member.path().join("myco.toml"),
                "project_dir = \"ledger\"\n",
            )
            .unwrap();
            let p =
                discover_in_index(member.path(), None, None, test_loader(), Some(&index)).unwrap();
            assert_eq!(p.root, member.path().canonicalize().unwrap());
            assert_eq!(p.worktree, ledger.canonicalize().unwrap());
        });
    }

    #[test]
    fn org_fallback_prefers_own_registered_worktree() {
        temp_env::with_var("MYCO_CONFIG", None::<&str>, || {
            let member = repo_without_project();
            // a custom ledger dir outside the repo root — the cwd
            // walk can't see it, the registry can (TODO.md qualifies)
            let custom = tempfile::tempdir().unwrap();
            std::fs::write(custom.path().join("TODO.md"), "# TODO\n").unwrap();
            let idx_dir = tempfile::tempdir().unwrap();
            let index = RedbIndex::open_at(
                camino::Utf8PathBuf::from_path_buf(idx_dir.path().join("idx")).unwrap(),
            )
            .unwrap();
            index
                .register(
                    &RepoEntry::new(canon_path(member.path()), "member")
                        .with_worktree(canon_path(custom.path()))
                        .with_ledger(LedgerMode::Local),
                )
                .unwrap();
            let p =
                discover_in_index(member.path(), None, None, test_loader(), Some(&index)).unwrap();
            assert_eq!(p.worktree, custom.path().canonicalize().unwrap());
            assert_eq!(p.root, member.path().canonicalize().unwrap());
        });
    }

    #[test]
    fn org_fallback_needs_a_valid_org_entry() {
        temp_env::with_var("MYCO_CONFIG", None::<&str>, || {
            let member = repo_without_project();
            let idx_dir = tempfile::tempdir().unwrap();
            let index = RedbIndex::open_at(
                camino::Utf8PathBuf::from_path_buf(idx_dir.path().join("idx")).unwrap(),
            )
            .unwrap();
            index
                .register(
                    &RepoEntry::new(canon_path(member.path()), "member")
                        .with_ledger(LedgerMode::None),
                )
                .unwrap();
            // no org entry designated → ordinary failure
            assert!(
                discover_in_index(member.path(), None, None, test_loader(), Some(&index)).is_err()
            );
            // an org entry whose worktree doesn't qualify → same
            let dead = repo_without_project();
            index
                .register(
                    &RepoEntry::new(canon_path(dead.path()), "dead").with_ledger(LedgerMode::Org),
                )
                .unwrap();
            assert!(
                discover_in_index(member.path(), None, None, test_loader(), Some(&index)).is_err()
            );
        });
    }
}
