//! `myco index add|remove|list|rebuild` — the derived-cache
//! registry verbs.
//!
//! `add` validates the repo has a `project/` worktree (same rules
//! as discovery), registers it, and takes a first snapshot.
//! `rebuild` re-scans via `MdStore` and re-keys the snapshot on the
//! project-branch HEAD — freshness afterwards is the trivial
//! `stored head == current head` check `org` does before trusting
//! a snapshot.

use std::path::{Path, PathBuf};

use camino::Utf8PathBuf;
use clap::Subcommand;
use myco_index::{Index, RepoEntry, RepoSnapshot};
use myco_index_redb::RedbIndex;
use serde_json::json;

use crate::{digest, discover, ops};

#[derive(Subcommand)]
pub enum Cmd {
    /// Register a repo — `<path>` is a repo root (or its
    /// `project/` worktree). Takes an initial snapshot.
    Add {
        /// Repo root (or project worktree) to register.
        path: PathBuf,
    },
    /// Unregister a repo and drop its snapshot.
    Remove {
        /// Registered repo root (or project worktree).
        path: PathBuf,
    },
    /// List registered repos with snapshot freshness.
    List,
    /// Re-scan a registered repo (all of them without `<path>`).
    Rebuild {
        /// Registered repo root (or project worktree).
        path: Option<PathBuf>,
    },
}

/// Open the default index (`$MYCO_HOME` / `~/.local/share/myco`).
pub fn open_index() -> Result<RedbIndex, String> {
    RedbIndex::open().map_err(|e| e.to_string())
}

/// RFC 3339 now — `scanned_at`.
pub fn now() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "unknown".into())
}

/// `<path>` → the repo root + project worktree. Accepts a repo
/// root (`<path>/project` must qualify) or the worktree itself.
/// `--dir` discovery doesn't qualify its path, so the check is
/// repeated here — `index add` on a plain dir must fail.
///
/// Both paths come back canonicalized — `root` keys the registry
/// and snapshots, so symlinked aliases of one repo must collapse
/// to a single entry (`remove`/`rebuild` canonicalize too).
pub fn repo_at(path: &Path) -> Result<discover::Project, String> {
    let mut proj = if let Ok(p) = discover::discover(None, Some(path)) {
        p
    } else {
        match discover::discover(Some(path), None) {
            Ok(p) if discover::qualifies(&p.worktree) => p,
            _ => {
                return Err(format!(
                    "{}: not a myco repo — needs a project/ worktree",
                    path.display()
                ));
            }
        }
    };
    proj.root = proj
        .root
        .canonicalize()
        .map_err(|e| format!("{}: {e}", proj.root.display()))?;
    proj.worktree = proj
        .worktree
        .canonicalize()
        .map_err(|e| format!("{}: {e}", proj.worktree.display()))?;
    Ok(proj)
}

/// Scan a repo's worktree into a snapshot.
pub fn scan_repo(proj: &discover::Project) -> Result<RepoSnapshot, String> {
    let store = ops::store_at(&proj.worktree)?;
    let scan = digest::scan(&store)?;
    let path = Utf8PathBuf::from_path_buf(proj.root.clone())
        .map_err(|p| format!("{}: non-utf8 path", p.display()))?;
    Ok(RepoSnapshot {
        path,
        name: scan.name,
        head_commit: myco_git::head_commit(&proj.worktree).unwrap_or_default(),
        scanned_at: now(),
        items: scan.items,
        archived: scan.archived,
    })
}

/// Freshness check — the entry's registered worktree HEAD
/// (`--dir`/`project_dir` worktrees don't live at
/// `<root>/project`). `None` when the worktree is gone or has no
/// HEAD. Used by `index list` and `org`'s auto-rebuild.
pub fn current_head(entry: &RepoEntry) -> Option<String> {
    myco_git::head_commit(entry.worktree_path().as_std_path())
}

/// Map `items` across scoped worker threads, preserving order —
/// for the per-repo `git`/fs calls that dominate `org`/`index`
/// latency. Keep redb I/O out of `f`: the index locks per open.
pub fn par_map<T: Sync, R: Send>(items: &[T], f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let workers = std::thread::available_parallelism()
        .map_or(4, |n| n.get())
        .min(16)
        .min(items.len().max(1));
    let next = AtomicUsize::new(0);
    let (tx, rx) = std::sync::mpsc::channel::<(usize, R)>();
    std::thread::scope(|s| {
        for _ in 0..workers {
            let tx = tx.clone();
            let (next, f) = (&next, &f);
            s.spawn(move || loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                if i >= items.len() {
                    return;
                }
                let _ = tx.send((i, f(&items[i])));
            });
        }
        drop(tx);
        let mut out: Vec<Option<R>> = items.iter().map(|_| None).collect();
        for (i, r) in rx {
            out[i] = Some(r);
        }
        // a hole means a worker died mid-map — `f` panicking also
        // propagates out of `thread::scope`, so this only fires
        // when the panic was already swallowed elsewhere
        out.into_iter()
            .enumerate()
            .map(|(i, o)| o.unwrap_or_else(|| panic!("par_map: no result for item {i}")))
            .collect()
    })
}

/// Re-scan a registered repo's worktree without the redb write —
/// fs + git only, safe to run in parallel. Callers persist via
/// `index.save_snapshot` serially (the index locks per open).
pub fn scan_one(entry: &RepoEntry) -> Result<RepoSnapshot, String> {
    let wt = entry.worktree_path();
    // a vanished (or never-was) worktree must error, not scan —
    // the store returns empty defaults for missing dirs, which
    // would overwrite a good snapshot with a phantom empty one
    if !discover::qualifies(wt.as_std_path()) {
        return Err(format!("{wt}: not a myco project worktree"));
    }
    let store = ops::store_at(wt.as_std_path())?;
    let scan = digest::scan(&store)?;
    Ok(RepoSnapshot {
        path: entry.path.clone(),
        name: scan.name,
        // "" when HEAD is unreadable — never matches a real head,
        // so the snapshot stays stale rather than looking fresh
        head_commit: myco_git::head_commit(wt.as_std_path()).unwrap_or_default(),
        scanned_at: now(),
        items: scan.items,
        archived: scan.archived,
    })
}

/// The registered entry matching `path`, else a "not registered"
/// error. `path` may be the repo root or the worktree.
fn registered_entry(index: &impl Index, path: &Path) -> Result<RepoEntry, String> {
    let proj = repo_at(path)?;
    let key = Utf8PathBuf::from_path_buf(proj.root.clone())
        .map_err(|p| format!("{}: non-utf8 path", p.display()))?;
    index
        .repos()
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|r| r.path == key)
        .ok_or_else(|| {
            format!(
                "{}: not registered — `myco index add` it first",
                proj.root.display()
            )
        })
}

/// Lexically normalize `p` — resolve `.`/`..` without touching
/// the fs (`canonicalize` needs the inode to exist, which a
/// deleted repo doesn't have).
fn normalize_path(p: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push(c);
                }
            }
            other => out.push(other),
        }
    }
    out
}

/// The registered repo path matching `path`, if any — the
/// registry decides the match, not the fs. Candidates, most
/// truthful first: discovery's canonical root, `canonicalize` on
/// the arg, the raw arg normalized (covers deleted worktrees —
/// `canonicalize` can't see them), and the parent of a trailing
/// `project/` arg.
fn registered_key(index: &impl Index, path: &Path) -> Result<Option<Utf8PathBuf>, String> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(p) = repo_at(path) {
        candidates.push(p.root);
    }
    if let Ok(c) = path.canonicalize() {
        candidates.push(c);
    }
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|e| format!("cwd: {e}"))?
            .join(path)
    };
    candidates.push(normalize_path(&abs));
    if abs.file_name().is_some_and(|n| n == discover::DIR) {
        if let Some(parent) = abs.parent() {
            candidates.push(normalize_path(parent));
        }
    }
    let repos = index.repos().map_err(|e| e.to_string())?;
    for cand in &candidates {
        let key = Utf8PathBuf::from_path_buf(normalize_path(cand)).ok();
        if let Some(hit) = repos.iter().find(|r| Some(&r.path) == key.as_ref()) {
            return Ok(Some(hit.path.clone()));
        }
    }
    Ok(None)
}

fn snap_json(s: &RepoSnapshot) -> serde_json::Value {
    json!({
        "path": s.path,
        "name": s.name,
        "head_commit": s.head_commit,
        "scanned_at": s.scanned_at,
        "items": s.items.len(),
        "archived": s.archived.len(),
    })
}

/// The `index` verb group.
pub fn run(cmd: Cmd, json_out: bool) -> Result<i32, String> {
    let index = open_index()?;
    match cmd {
        Cmd::Add { path } => {
            let proj = repo_at(&path)?;
            let snap = scan_repo(&proj)?;
            // root + worktree are canonicalized by repo_at — the
            // registered key and the snapshot key agree, and the
            // worktree path rides along for `current_head`/`rebuild`
            let wt = Utf8PathBuf::from_path_buf(proj.worktree.clone())
                .map_err(|p| format!("{}: non-utf8 path", p.display()))?;
            let entry = RepoEntry::new(snap.path.clone(), snap.name.clone()).with_worktree(wt);
            index.register(&entry).map_err(|e| e.to_string())?;
            index.save_snapshot(&snap).map_err(|e| e.to_string())?;
            if json_out {
                ops::print_json(&json!({"registered": entry, "snapshot": snap_json(&snap)}));
            } else {
                println!(
                    "registered {} ({}) — {} items @ {}",
                    snap.name,
                    snap.path,
                    snap.items.len(),
                    &snap.head_commit[..snap.head_commit.len().min(7)],
                );
            }
            Ok(0)
        }
        Cmd::Remove { path } => {
            // the registry decides the key — a repo whose
            // worktree (or whole dir) vanished can't canonicalize,
            // so `registered_key` also matches the raw arg's
            // normalized forms against what's actually registered
            let Some(key) = registered_key(&index, &path)? else {
                return Err(format!("{}: not registered", path.display()));
            };
            if index.remove(&key).map_err(|e| e.to_string())? {
                if json_out {
                    ops::print_json(&json!({"removed": key}));
                } else {
                    println!("removed {key}");
                }
                Ok(0)
            } else {
                Err(format!("{key}: not registered"))
            }
        }
        Cmd::List => {
            let repos = index.repos().map_err(|e| e.to_string())?;
            let heads = par_map(&repos, current_head);
            let mut rows = Vec::new();
            let mut out = Vec::new();
            for (r, head) in repos.iter().zip(&heads) {
                let snap = index.snapshot(&r.path).map_err(|e| e.to_string())?;
                let stale = match (&snap, head) {
                    (Some(s), Some(h)) => s.head_commit != *h,
                    (Some(_), None) => true, // worktree gone
                    (None, _) => true,       // never scanned
                };
                out.push(json!({
                    "path": r.path,
                    "name": r.name,
                    "head_commit": snap.as_ref().map(|s| s.head_commit.clone()),
                    "scanned_at": snap.as_ref().map(|s| s.scanned_at.clone()),
                    "items": snap.as_ref().map(|s| s.items.len()),
                    "stale": stale,
                }));
                rows.push(format!(
                    "{}\t{}\t{}\t{}",
                    r.path,
                    r.name,
                    snap.as_ref()
                        .map(|s| s.items.len().to_string())
                        .unwrap_or_else(|| "-".into()),
                    if stale { "stale" } else { "fresh" },
                ));
            }
            if json_out {
                ops::print_json(&json!(out));
            } else if rows.is_empty() {
                println!("no repos registered — `myco index add <path>`");
            } else {
                ops::print_paged(&rows.join("\n"));
            }
            Ok(0)
        }
        Cmd::Rebuild { path } => {
            let entries: Vec<RepoEntry> = match path {
                Some(p) => vec![registered_entry(&index, &p)?],
                None => index.repos().map_err(|e| e.to_string())?,
            };
            if entries.is_empty() {
                if json_out {
                    ops::print_json(&json!([]));
                } else {
                    println!("no repos registered — `myco index add <path>`");
                }
                return Ok(0);
            }
            // parallel rescans; serial saves + ordered reporting
            let snaps = par_map(&entries, scan_one);
            let mut results = Vec::new();
            let mut failures = Vec::new();
            for (entry, snap) in entries.iter().zip(snaps) {
                match snap.and_then(|s| {
                    index
                        .save_snapshot(&s)
                        .map_err(|e| e.to_string())
                        .map(|_| s)
                }) {
                    Ok(s) => {
                        if !json_out {
                            println!(
                                "rebuilt {} — {} items @ {}",
                                s.name,
                                s.items.len(),
                                &s.head_commit[..s.head_commit.len().min(7)],
                            );
                        }
                        results.push(
                            json!({"path": s.path, "status": "rebuilt", "snapshot": snap_json(&s)}),
                        );
                    }
                    Err(e) => {
                        failures.push(format!("{}: {e}", entry.path));
                        results.push(json!({"path": entry.path, "status": "error", "error": e}));
                    }
                }
            }
            if json_out {
                ops::print_json(&json!(results));
            }
            if failures.is_empty() {
                Ok(0)
            } else {
                Err(format!(
                    "{} rebuild(s) failed: {}",
                    failures.len(),
                    failures.join("; ")
                ))
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use myco_index_redb::RedbIndex;
    use std::process::Command;

    fn idx() -> (tempfile::TempDir, RedbIndex) {
        let d = tempfile::tempdir().unwrap();
        let dir = Utf8PathBuf::from_path_buf(d.path().join("idx")).unwrap();
        (d, RedbIndex::open_at(dir).unwrap())
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

    /// A repo with a `project/` worktree on the `project` branch.
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
    fn normalize_path_resolves_dots_lexically() {
        assert_eq!(
            normalize_path(Path::new("/a/b/../c")),
            PathBuf::from("/a/c")
        );
        assert_eq!(normalize_path(Path::new("/a/./b")), PathBuf::from("/a/b"));
        // a leading `..` with nothing to pop is kept
        assert_eq!(normalize_path(Path::new("../x")), PathBuf::from("../x"));
        assert_eq!(
            normalize_path(Path::new("/a/b/../../c")),
            PathBuf::from("/c")
        );
    }

    #[test]
    fn registered_key_finds_deleted_repo() {
        let (_d, i) = idx();
        // registered path that no longer exists — canonicalize
        // can't see it, the normalized arg still matches
        let gone = Utf8PathBuf::from("/totally/gone/repo-xyz");
        i.register(&RepoEntry::new(gone.clone(), "gone")).unwrap();
        assert_eq!(
            registered_key(&i, Path::new("/totally/gone/repo-xyz")).unwrap(),
            Some(gone.clone())
        );
        // pointing at the dead worktree resolves to the repo root
        assert_eq!(
            registered_key(&i, Path::new("/totally/gone/repo-xyz/project")).unwrap(),
            Some(gone)
        );
        // unregistered stays unregistered
        assert_eq!(
            registered_key(&i, Path::new("/totally/gone/other")).unwrap(),
            None
        );
    }

    #[test]
    fn add_canonicalizes_and_remove_survives_deletion() {
        let idx_dir = tempfile::tempdir().unwrap();
        let ds = idx_dir.path().to_str().unwrap().to_string();
        temp_env::with_var("MYCO_HOME", Some(ds.as_str()), || {
            let d = repo_with_project();
            let canon_root = Utf8PathBuf::from_path_buf(d.path().canonicalize().unwrap()).unwrap();
            run(
                Cmd::Add {
                    path: d.path().to_path_buf(),
                },
                true,
            )
            .unwrap();
            let index = RedbIndex::open().unwrap();
            let repos = index.repos().unwrap();
            assert_eq!(repos.len(), 1);
            // registered key is the canonical root; the worktree
            // path rides along for freshness checks
            assert_eq!(repos[0].path, canon_root);
            assert_eq!(repos[0].worktree_path(), canon_root.join(discover::DIR));
            // the whole repo vanishes — remove must still match
            std::fs::remove_dir_all(d.path().join("project")).unwrap();
            run(
                Cmd::Remove {
                    path: d.path().join("project"),
                },
                true,
            )
            .unwrap();
            assert!(index.repos().unwrap().is_empty());
        });
    }
}
