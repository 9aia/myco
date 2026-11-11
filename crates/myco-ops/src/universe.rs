//! The index universe — which repos index-sourced views cover
//! (P-026).
//!
//! Inside a **lasius workspace** (`lasius.toml` found walking up
//! from the anchor — lasius-cli's own discovery rule) the
//! universe is the workspace's clones: `<root>/<repos_dir>/*`
//! dirs containing `.git`, minus `exclude` globs — unioned with
//! `repos.toml` registrations whose repo isn't a clone (they
//! still appear, marked [`RepoSource::Registry`]). Outside
//! lasius the universe is the registry alone, exactly as before.
//!
//! Per repo, the ledger designation resolves: the repo's own
//! `myco.toml` `[index] ledger` wins, then an explicit legacy
//! `repos.toml` `ledger` (`org`/`none` — `local` is the default,
//! not a mark), then derivation for lasius clones (a qualifying
//! worktree → `local`, none → `none`). Registry-only entries
//! keep their registered ledger unless `myco.toml` overrides —
//! re-deriving them would silently reclassify repos whose
//! worktree merely moved.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use camino::Utf8PathBuf;
use myco_index::{Index, LedgerMode, RepoEntry};

use crate::config::ConfigLoader;
use crate::discover;

/// Where a universe repo came from — `index list` marks
/// registry extras inside a lasius workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepoSource {
    /// A lasius workspace clone.
    Lasius,
    /// A `repos.toml` registration.
    Registry,
}

impl RepoSource {
    /// Stable label for `--json` output.
    pub fn as_str(self) -> &'static str {
        match self {
            RepoSource::Lasius => "lasius",
            RepoSource::Registry => "registry",
        }
    }
}

/// One universe repo — the effective registry entry plus its
/// origin.
#[derive(Debug, Clone)]
pub struct UniverseRepo {
    /// The entry views/rebuilds consume — worktree and ledger
    /// already resolved against `myco.toml` + the registry.
    pub entry: RepoEntry,
    /// `lasius` clone vs `registry` (`repos.toml`) origin.
    pub source: RepoSource,
}

/// `(root, config)` of the lasius workspace containing `anchor`,
/// walking ancestors for `lasius.toml` — the same rule
/// lasius-cli's `resolve` applies. `None` outside a workspace.
fn lasius_workspace(anchor: &Path) -> Option<(PathBuf, lasius_core::WorkspaceConfig)> {
    let anchor = anchor
        .canonicalize()
        .unwrap_or_else(|_| anchor.to_path_buf());
    for d in anchor.ancestors() {
        let cfg_path = d.join("lasius.toml");
        if cfg_path.is_file() {
            let text = std::fs::read_to_string(&cfg_path).ok()?;
            let config = lasius_core::WorkspaceConfig::from_toml(&text).ok()?;
            return Some((d.to_path_buf(), config));
        }
    }
    None
}

/// Clone dirs under `<root>/<repos_dir>` — lasius-cli's
/// `local_repos` rule: real dirs (no symlink follow) containing
/// `.git`, sorted, `exclude` globs honored.
fn clones(root: &Path, config: &lasius_core::WorkspaceConfig) -> Vec<PathBuf> {
    let dir = config.repos_path(root);
    let Ok(rd) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for e in rd.flatten() {
        let Ok(ft) = e.file_type() else {
            continue;
        };
        if !ft.is_dir() {
            continue;
        }
        let name = e.file_name().to_string_lossy().into_owned();
        if config.is_excluded(&name) {
            continue;
        }
        let p = e.path();
        if p.join(".git").exists() {
            out.push(p);
        }
    }
    out.sort();
    out
}

/// `[index] ledger` from the repo's own `myco.toml` — the
/// repo-layer file only, read directly (never the layered
/// search): a user-layer `[index]` must not designate every
/// repo on the machine. Missing/broken config → `None`.
fn config_ledger(root: &Path, cfg: ConfigLoader) -> Option<LedgerMode> {
    let f = root.join("myco.toml");
    if !f.is_file() {
        return None;
    }
    cfg.load_file(&f)
        .ok()
        .and_then(|c| c.index)
        .and_then(|i| i.ledger)
}

/// The clone's ledger worktree — a registered worktree that
/// still qualifies wins (explicit registration beats
/// discovery), else myco discovery at the clone root
/// (`project_dir`-aware, conventional `project/` fallback).
fn clone_worktree(
    root: &Path,
    registered: Option<&RepoEntry>,
    cfg: ConfigLoader,
) -> Option<PathBuf> {
    if let Some(w) = registered.and_then(|r| r.worktree_path()) {
        let w = w.as_std_path();
        if discover::qualifies(w) {
            return Some(w.to_path_buf());
        }
    }
    discover::discover_at(root, None, None, cfg).ok().map(|p| {
        p.worktree
            .canonicalize()
            .unwrap_or_else(|_| p.worktree.clone())
    })
}

/// The effective index universe seen from `anchor` (a cwd or
/// repo path — lasius discovery walks its ancestors).
///
/// Registry entries merge into lasius clones by canonical repo
/// root: the clone supplies presence + fresh worktree/ledger
/// resolution, the registry row still contributes an explicit
/// `org`/`none` mark and a registered custom worktree.
pub fn repos(
    anchor: &Path,
    index: &dyn Index,
    cfg: ConfigLoader,
) -> Result<Vec<UniverseRepo>, String> {
    let registered = index.repos().map_err(|e| e.to_string())?;
    let by_path: BTreeMap<PathBuf, RepoEntry> = registered
        .iter()
        .map(|r| {
            (
                r.path
                    .as_std_path()
                    .canonicalize()
                    .unwrap_or_else(|_| r.path.as_std_path().to_path_buf()),
                r.clone(),
            )
        })
        .collect();

    let mut out: Vec<UniverseRepo> = Vec::new();
    let mut covered: Vec<PathBuf> = Vec::new();

    if let Some((ws_root, ws)) = lasius_workspace(anchor) {
        for dir in clones(&ws_root, &ws) {
            let root = dir.canonicalize().unwrap_or(dir);
            covered.push(root.clone());
            let reg = by_path.get(&root);
            let worktree = clone_worktree(&root, reg, cfg);
            let ledger = config_ledger(&root, cfg)
                .or_else(|| reg.map(|r| r.ledger).filter(|l| !l.is_local()))
                .unwrap_or(if worktree.is_some() {
                    LedgerMode::Local
                } else {
                    LedgerMode::None
                });
            let name = reg.map(|r| r.name.clone()).unwrap_or_else(|| {
                root.file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "repo".into())
            });
            let key = Utf8PathBuf::from_path_buf(root.clone())
                .map_err(|p| format!("{}: non-utf8 path", p.display()))?;
            let entry = RepoEntry::new(key, name).with_ledger(ledger).with_worktree(
                worktree
                    .and_then(|w| Utf8PathBuf::from_path_buf(w).ok())
                    .unwrap_or_default(),
            );
            out.push(UniverseRepo {
                entry,
                source: RepoSource::Lasius,
            });
        }
    }

    for r in registered {
        let key = r
            .path
            .as_std_path()
            .canonicalize()
            .unwrap_or_else(|_| r.path.as_std_path().to_path_buf());
        if covered.contains(&key) {
            continue;
        }
        // registry-only — the repo's own `myco.toml` `[index]`
        // still designates; otherwise the registered ledger stands
        let mut entry = r;
        if let Some(l) = config_ledger(entry.path.as_std_path(), cfg) {
            entry.ledger = l;
        }
        out.push(UniverseRepo {
            entry,
            source: RepoSource::Registry,
        });
    }

    out.sort_by(|a, b| a.entry.path.cmp(&b.entry.path));
    Ok(out)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::config::test_loader;
    use myco_index_redb::RedbIndex;
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

    /// A repo root with a qualifying `project/` worktree.
    fn repo_with_project(dir: &Path, name: &str) -> PathBuf {
        let r = dir.join(name);
        std::fs::create_dir_all(&r).unwrap();
        git(&r, &["init", "-q"]);
        git(&r, &["config", "user.email", "t@t"]);
        git(&r, &["config", "user.name", "t"]);
        crate::vcs::create_orphan_branch(&r, "project", "init").unwrap();
        git(&r, &["worktree", "add", "project", "project"]);
        std::fs::write(r.join("project/TODO.md"), "# TODO — x\n").unwrap();
        r
    }

    /// A lasius workspace: `<tmp>/lasius.toml` + `repos/` clones.
    fn lasius_ws() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("lasius.toml"), "org = \"o\"\n").unwrap();
        std::fs::create_dir_all(d.path().join("repos")).unwrap();
        d
    }

    fn idx(d: &Path) -> RedbIndex {
        let dir = Utf8PathBuf::from_path_buf(d.join("idx")).unwrap();
        RedbIndex::open_at(dir).unwrap()
    }

    #[test]
    fn outside_lasius_universe_is_the_registry() {
        let d = tempfile::tempdir().unwrap();
        let index = idx(d.path());
        let r = repo_with_project(d.path(), "repo");
        let key = Utf8PathBuf::from_path_buf(r.canonicalize().unwrap()).unwrap();
        index.register(&RepoEntry::new(key, "repo")).unwrap();
        let uni = repos(d.path(), &index, test_loader()).unwrap();
        assert_eq!(uni.len(), 1);
        assert_eq!(uni[0].source, RepoSource::Registry);
        assert_eq!(uni[0].entry.ledger, LedgerMode::Local);
        assert!(uni[0].entry.worktree_path().is_some());
    }

    #[test]
    fn lasius_clones_join_without_registration() {
        let ws = lasius_ws();
        let a = repo_with_project(&ws.path().join("repos"), "aaa");
        let b = repo_with_project(&ws.path().join("repos"), "bbb");
        let index = idx(ws.path());
        // anchor deep inside a clone — ancestors still find lasius.toml
        let uni = repos(&a.join("project"), &index, test_loader()).unwrap();
        assert_eq!(uni.len(), 2);
        assert!(uni.iter().all(|u| u.source == RepoSource::Lasius));
        let names: Vec<&str> = uni.iter().map(|u| u.entry.name.as_str()).collect();
        assert_eq!(names, ["aaa", "bbb"]);
        for u in &uni {
            assert_eq!(u.entry.ledger, LedgerMode::Local);
            assert!(u.entry.worktree_path().is_some());
        }
        let _ = b;
    }

    #[test]
    fn ledgerless_clone_derives_none() {
        let ws = lasius_ws();
        // a plain git clone — no project/ worktree
        let r = ws.path().join("repos/plain");
        std::fs::create_dir_all(&r).unwrap();
        git(&r, &["init", "-q"]);
        git(&r, &["config", "user.email", "t@t"]);
        git(&r, &["config", "user.name", "t"]);
        let index = idx(ws.path());
        let uni = repos(ws.path(), &index, test_loader()).unwrap();
        assert_eq!(uni.len(), 1);
        assert_eq!(uni[0].entry.ledger, LedgerMode::None);
        assert_eq!(uni[0].entry.worktree_path(), None);
    }

    #[test]
    fn myco_toml_index_ledger_wins() {
        let ws = lasius_ws();
        let org = repo_with_project(&ws.path().join("repos"), "orgrepo");
        std::fs::write(org.join("myco.toml"), "[index]\nledger = \"org\"\n").unwrap();
        let member = repo_with_project(&ws.path().join("repos"), "member");
        std::fs::write(member.join("myco.toml"), "[index]\nledger = \"none\"\n").unwrap();
        let index = idx(ws.path());
        let uni = repos(ws.path(), &index, test_loader()).unwrap();
        let by_name = |n: &str| uni.iter().find(|u| u.entry.name == n).unwrap();
        assert_eq!(by_name("orgrepo").entry.ledger, LedgerMode::Org);
        // explicit none wins over the qualifying worktree
        assert_eq!(by_name("member").entry.ledger, LedgerMode::None);
        assert_eq!(by_name("member").entry.worktree_path(), None);
    }

    #[test]
    fn legacy_registry_ledger_and_extras_survive() {
        let ws = lasius_ws();
        let clone = repo_with_project(&ws.path().join("repos"), "clone");
        // a registered repo outside the workspace still appears
        let outside = repo_with_project(ws.path(), "outside");
        let index = idx(ws.path());
        let okey = Utf8PathBuf::from_path_buf(outside.canonicalize().unwrap()).unwrap();
        index
            .register(&RepoEntry::new(okey, "outside").with_ledger(LedgerMode::Org))
            .unwrap();
        // and a legacy `none` mark on the clone itself is honored
        let ckey = Utf8PathBuf::from_path_buf(clone.canonicalize().unwrap()).unwrap();
        index
            .register(&RepoEntry::new(ckey, "clone").with_ledger(LedgerMode::None))
            .unwrap();
        let uni = repos(ws.path(), &index, test_loader()).unwrap();
        assert_eq!(uni.len(), 2);
        let clone_u = uni.iter().find(|u| u.entry.name == "clone").unwrap();
        assert_eq!(clone_u.source, RepoSource::Lasius);
        assert_eq!(clone_u.entry.ledger, LedgerMode::None);
        let out_u = uni.iter().find(|u| u.entry.name == "outside").unwrap();
        assert_eq!(out_u.source, RepoSource::Registry);
        assert_eq!(out_u.entry.ledger, LedgerMode::Org);
    }

    #[test]
    fn non_clone_dirs_and_excludes_are_skipped() {
        let ws = lasius_ws();
        let _ = repo_with_project(&ws.path().join("repos"), "real");
        // not a git repo → not a clone
        std::fs::create_dir_all(ws.path().join("repos/notgit")).unwrap();
        std::fs::write(
            ws.path().join("lasius.toml"),
            "org = \"o\"\nexclude = [\"skip-*\"]\n",
        )
        .unwrap();
        let _ = repo_with_project(&ws.path().join("repos"), "skip-me");
        let index = idx(ws.path());
        let uni = repos(ws.path(), &index, test_loader()).unwrap();
        assert_eq!(uni.len(), 1);
        assert_eq!(uni[0].entry.name, "real");
    }
}
