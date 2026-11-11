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

use camino::{Utf8Path, Utf8PathBuf};
use myco_index::{Index, LedgerMode, RepoEntry, RepoSnapshot};
use myco_index_redb::RedbIndex;
use serde_json::json;

use crate::registry::{Args, Ctx};
use crate::{digest, discover, links, ops};

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
/// root (`<path>/project` must qualify), the worktree itself, or
/// — for `index add`'s ledger-less path — any dir inside a git
/// repo (the returned worktree is the conventional
/// `<root>/project`, which may not exist; callers check
/// [`discover::qualifies`]). `--dir` discovery doesn't qualify
/// its path, so the check is repeated here — `index add` on a
/// plain dir must fail.
///
/// Both paths come back canonicalized — `root` keys the registry
/// and snapshots, so symlinked aliases of one repo must collapse
/// to a single entry (`remove`/`rebuild` canonicalize too).
pub fn repo_at(path: &Path) -> Result<discover::Project, String> {
    // strict discovery — the cli wrapper's org-ledger fallback
    // must never answer here: `index add <member>` registering
    // the org ledger's worktree under the member's key would be
    // a silent wrong-repo write
    let strict = |dir: Option<&Path>, root: Option<&Path>| {
        myco_ops::discover::discover(dir, root, crate::ops::CONFIG)
    };
    if let Ok(mut proj) = strict(None, Some(path)) {
        proj.root = proj
            .root
            .canonicalize()
            .map_err(|e| format!("{}: {e}", proj.root.display()))?;
        proj.worktree = proj
            .worktree
            .canonicalize()
            .map_err(|e| format!("{}: {e}", proj.worktree.display()))?;
        return Ok(proj);
    }
    if let Ok(p) = strict(Some(path), None) {
        if discover::qualifies(&p.worktree) {
            let mut proj = p;
            proj.root = proj
                .root
                .canonicalize()
                .map_err(|e| format!("{}: {e}", proj.root.display()))?;
            proj.worktree = proj
                .worktree
                .canonicalize()
                .map_err(|e| format!("{}: {e}", proj.worktree.display()))?;
            return Ok(proj);
        }
    }
    // ledger-less: any git repo registers without a snapshot —
    // `h_add` reads the registry to tell "no ledger yet" from
    // "custom worktree still qualifies"
    let repo = myco_ops::vcs::repo_root(path)
        .ok()
        .or_else(|| path.parent().and_then(|p| myco_ops::vcs::repo_root(p).ok()))
        .ok_or_else(|| {
            format!(
                "{}: not a myco repo — not inside a git repository",
                path.display()
            )
        })?;
    let root = myco_ops::vcs::main_worktree(&repo).unwrap_or(repo);
    let root = root
        .canonicalize()
        .map_err(|e| format!("{}: {e}", root.display()))?;
    let worktree = root.join(discover::DIR);
    // an existing-but-invalid project/ dir is the init-time
    // conflict, not a ledger-less repo — refuse like `init` does
    if worktree.exists() && !discover::qualifies(&worktree) {
        return Err(format!(
            "{} exists but is not a myco project worktree — \
             move it away or run `myco init`",
            worktree.display()
        ));
    }
    Ok(discover::Project { root, worktree })
}

/// Scan a repo's worktree into a snapshot.
pub fn scan_repo(proj: &discover::Project) -> Result<RepoSnapshot, String> {
    let store = ops::store_at(&proj.worktree, myco_core::kind::prelude_table())?;
    let scan = digest::scan(&store)?;
    let path = Utf8PathBuf::from_path_buf(proj.root.clone())
        .map_err(|p| format!("{}: non-utf8 path", p.display()))?;
    Ok(RepoSnapshot {
        path,
        name: scan.name,
        head_commit: myco_ops::vcs::head_commit(&proj.worktree).unwrap_or_default(),
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
    entry
        .worktree_path()
        .and_then(|w| myco_ops::vcs::head_commit(w.as_std_path()))
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
    let Some(wt) = entry.worktree_path() else {
        return Err(format!("{}: registered without a ledger", entry.path));
    };
    // a vanished (or never-was) worktree must error, not scan —
    // the store returns empty defaults for missing dirs, which
    // would overwrite a good snapshot with a phantom empty one
    if !discover::qualifies(wt.as_std_path()) {
        return Err(format!("{wt}: not a myco project worktree"));
    }
    let store = ops::store_at(wt.as_std_path(), myco_core::kind::prelude_table())?;
    let scan = digest::scan(&store)?;
    Ok(RepoSnapshot {
        path: entry.path.clone(),
        name: scan.name,
        // "" when HEAD is unreadable — never matches a real head,
        // so the snapshot stays stale rather than looking fresh
        head_commit: myco_ops::vcs::head_commit(wt.as_std_path()).unwrap_or_default(),
        scanned_at: now(),
        items: scan.items,
        archived: scan.archived,
    })
}

/// The link index rides the same scan (P-005): edges for this
/// repo's `project/` files, invalidated by fingerprint. `wt` is
/// the entry's registered worktree — non-utf8 paths skip links
/// rather than failing the snapshot rebuild.
fn sync_links(
    index: &impl myco_index::LinkIndex,
    repo: &str,
    wt: &Utf8Path,
) -> Result<myco_ops::links::SyncReport, String> {
    myco_ops::links::sync(index, repo, wt, |rel, src| {
        links::extract_file(rel, src, myco_core::kind::prelude_table())
    })
}

/// The universe entry matching `path`, else a "not in the
/// universe" error. `path` may be the repo root or the worktree.
fn universe_entry(
    uni: &[myco_ops::universe::UniverseRepo],
    path: &Path,
) -> Result<RepoEntry, String> {
    let proj = repo_at(path)?;
    let key = Utf8PathBuf::from_path_buf(proj.root.clone())
        .map_err(|p| format!("{}: non-utf8 path", p.display()))?;
    uni.iter()
        .find(|u| u.entry.path == key)
        .map(|u| u.entry.clone())
        .ok_or_else(|| {
            format!(
                "{}: not in the index universe — `myco index add` it first",
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

/// The `index add` verb — registers the repo and takes a first
/// snapshot *when it has a ledger*. A git repo with no qualifying
/// `project/` worktree registers as `ledger = "none"` (the org
/// pattern's explicit ledger-less state) with no snapshot;
/// `--org` marks the repo the org/global ledger instead — the
/// fallback target discovery resolves ledger-less members to —
/// and clears the mark on every other entry (at most one).
pub fn h_add(ctx: &Ctx, a: &Args) -> Result<i32, String> {
    let index = open_index()?;
    let json_out = ctx.json;
    let org_flag = a.flag("org");
    let path = PathBuf::from(a.text("path")?);
    {
        let proj = repo_at(&path)?;
        let root_key = Utf8PathBuf::from_path_buf(proj.root.clone())
            .map_err(|p| format!("{}: non-utf8 path", p.display()))?;
        let existing = index
            .repos()
            .map_err(|e| e.to_string())?
            .into_iter()
            .find(|r| r.path == root_key);

        // the observed ledger: the discovered/conventional
        // worktree when it qualifies, else a registered custom
        // worktree that still does — `none` when neither does
        let observed = if discover::qualifies(&proj.worktree) {
            Some(proj.worktree.clone())
        } else {
            existing
                .as_ref()
                .and_then(|e| e.worktree_path())
                .map(|w| w.as_std_path().to_path_buf())
                .filter(|w| discover::qualifies(w))
        };
        let ledger = if org_flag {
            if observed.is_none() {
                return Err(format!(
                    "{}: --org needs a repo with a project/ worktree",
                    proj.root.display()
                ));
            }
            LedgerMode::Org
        } else if observed.is_none() {
            LedgerMode::None
        } else if existing.is_some_and(|e| e.ledger == LedgerMode::Org) {
            // re-adding the org repo keeps its designation
            LedgerMode::Org
        } else {
            LedgerMode::Local
        };

        match observed {
            Some(wt) => {
                // root + worktree are canonicalized by repo_at —
                // the registered key and the snapshot key agree,
                // and the worktree path rides along for
                // `current_head`/`rebuild`
                let wt_u8 = Utf8PathBuf::from_path_buf(wt.clone())
                    .map_err(|p| format!("{}: non-utf8 path", p.display()))?;
                let proj = discover::Project {
                    root: proj.root.clone(),
                    worktree: wt,
                };
                let snap = scan_repo(&proj)?;
                let entry = RepoEntry::new(snap.path.clone(), snap.name.clone())
                    .with_worktree(wt_u8.clone())
                    .with_ledger(ledger);
                index.register(&entry).map_err(|e| e.to_string())?;
                index.save_snapshot(&snap).map_err(|e| e.to_string())?;
                // the link index rides the same scan (P-005)
                let lrep = sync_links(&index, snap.path.as_str(), &wt_u8)?;
                if json_out {
                    ops::print_json(&json!({"registered": entry, "snapshot": snap_json(&snap),
                            "links": {"files": lrep.files, "edges": lrep.edges}}));
                } else {
                    println!(
                        "registered {} ({}) — {} items @ {}, {} edges",
                        snap.name,
                        snap.path,
                        snap.items.len(),
                        &snap.head_commit[..snap.head_commit.len().min(7)],
                        lrep.edges,
                    );
                }
            }
            None => {
                // ledger-less — the dir basename is the name
                // (`meta.toml` lives in the ledger that isn't
                // there)
                let name = proj
                    .root
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "repo".into());
                let entry = RepoEntry::new(root_key.clone(), name).with_ledger(ledger);
                index.register(&entry).map_err(|e| e.to_string())?;
                if json_out {
                    ops::print_json(&json!({"registered": entry, "snapshot": null}));
                } else {
                    println!("registered {} ({}) — no ledger", entry.name, entry.path);
                }
            }
        }

        // at most one org ledger — designating clears the mark
        // everywhere else
        if ledger == LedgerMode::Org {
            for r in index.repos().map_err(|e| e.to_string())? {
                if r.ledger == LedgerMode::Org && r.path != root_key {
                    index
                        .register(&r.with_ledger(LedgerMode::Local))
                        .map_err(|e| e.to_string())?;
                }
            }
        }
        Ok(0)
    }
}

/// The `index remove` verb.
pub fn h_remove(ctx: &Ctx, a: &Args) -> Result<i32, String> {
    let index = open_index()?;
    let json_out = ctx.json;
    let path = PathBuf::from(a.text("path")?);
    {
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
}

/// The `index list` verb — the index *universe* (lasius clones
/// ∪ `repos.toml` registrations, P-026), each row marked with
/// its source when a lasius workspace is in play.
pub fn h_list(ctx: &Ctx, _a: &Args) -> Result<i32, String> {
    let index = open_index()?;
    let json_out = ctx.json;
    {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let uni = myco_ops::universe::repos(&cwd, &index, crate::ops::CONFIG)?;
        let in_lasius = uni
            .iter()
            .any(|u| u.source == myco_ops::universe::RepoSource::Lasius);
        let heads = par_map(&uni, |u| current_head(&u.entry));
        let mut rows = Vec::new();
        let mut out = Vec::new();
        for (u, head) in uni.iter().zip(&heads) {
            let r = &u.entry;
            let snap = index.snapshot(&r.path).map_err(|e| e.to_string())?;
            // ledger-less entries hold no snapshot by design —
            // "no-ledger", never "stale"
            let no_ledger = !r.has_ledger();
            let stale = !no_ledger
                && match (&snap, head) {
                    (Some(s), Some(h)) => s.head_commit != *h,
                    (Some(_), None) => true, // worktree gone
                    (None, _) => true,       // never scanned
                };
            out.push(json!({
                "path": r.path,
                "name": r.name,
                "ledger": r.ledger,
                "source": u.source.as_str(),
                "head_commit": snap.as_ref().map(|s| s.head_commit.clone()),
                "scanned_at": snap.as_ref().map(|s| s.scanned_at.clone()),
                "items": snap.as_ref().map(|s| s.items.len()),
                "stale": stale,
            }));
            rows.push(format!(
                "{}\t{}\t{}\t{}{}",
                r.path,
                r.name,
                snap.as_ref()
                    .map(|s| s.items.len().to_string())
                    .unwrap_or_else(|| "-".into()),
                if no_ledger {
                    "no-ledger".to_string()
                } else if stale {
                    "stale".to_string()
                } else {
                    "fresh".to_string()
                },
                // inside a lasius workspace every row is marked —
                // clones say `lasius`, registry extras `registry`
                if in_lasius {
                    format!("\t{}", u.source.as_str())
                } else {
                    String::new()
                },
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
}

/// The `index rebuild` verb.
pub fn h_rebuild(ctx: &Ctx, a: &Args) -> Result<i32, String> {
    let index = open_index()?;
    let json_out = ctx.json;
    let path = a.opt("path").map(PathBuf::from);
    {
        // the universe — lasius clones ∪ registry (P-026); a
        // path arg resolves against it the same way
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let uni = myco_ops::universe::repos(&cwd, &index, crate::ops::CONFIG)?;
        let entries: Vec<RepoEntry> = match path {
            Some(p) => vec![universe_entry(&uni, &p)?],
            None => uni.into_iter().map(|u| u.entry).collect(),
        };
        if entries.is_empty() {
            if json_out {
                ops::print_json(&json!([]));
            } else {
                println!("no repos registered — `myco index add <path>`");
            }
            return Ok(0);
        }
        // ledger-less entries hold no snapshot by design —
        // reported as `no-ledger`, never scanned, never failed
        let (skipped, scannable): (Vec<RepoEntry>, Vec<RepoEntry>) =
            entries.into_iter().partition(|e| !e.has_ledger());
        if !json_out {
            for e in &skipped {
                println!("{} — no ledger (skipped)", e.path);
            }
        }
        let mut results: Vec<serde_json::Value> = skipped
            .iter()
            .map(|e| json!({"path": e.path, "status": "no-ledger"}))
            .collect();
        // parallel rescans; serial saves + ordered reporting
        let snaps = par_map(&scannable, scan_one);
        let mut failures = Vec::new();
        for (entry, snap) in scannable.iter().zip(snaps) {
            match snap.and_then(|s| {
                index
                    .save_snapshot(&s)
                    .map_err(|e| e.to_string())
                    .map(|_| s)
                    // the link index rides the same rebuild
                    // (P-005): edges re-derived per dirty file —
                    // `scannable` entries all have a worktree
                    .and_then(|s| match entry.worktree_path() {
                        Some(wt) => sync_links(&index, s.path.as_str(), &wt).map(|l| (s, l)),
                        None => Ok((s, Default::default())),
                    })
            }) {
                Ok((s, lrep)) => {
                    if !json_out {
                        println!(
                            "rebuilt {} — {} items @ {}, {} edges",
                            s.name,
                            s.items.len(),
                            &s.head_commit[..s.head_commit.len().min(7)],
                            lrep.edges,
                        );
                    }
                    results.push(
                        json!({"path": s.path, "status": "rebuilt", "snapshot": snap_json(&s),
                                "links": {"files": lrep.files, "dirty": lrep.dirty,
                                    "swept": lrep.swept, "edges": lrep.edges}}),
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

    /// Drive a verb the way dispatch does — argv through the
    /// registry's decoder into the handler.
    fn run_verb(name: &str, argv: &[String]) -> Result<i32, String> {
        let mut ctx = crate::registry::Ctx {
            json: true,
            ..Default::default()
        };
        let v = crate::registry::find(name).unwrap();
        let a = crate::registry::Args::from_argv(v, argv, &mut ctx, false)?;
        (v.cli.unwrap())(&ctx, &a)
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
        myco_ops::vcs::create_orphan_branch(r, "project", "init").unwrap();
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
            run_verb("index.add", &[d.path().to_string_lossy().into_owned()]).unwrap();
            let index = RedbIndex::open().unwrap();
            let repos = index.repos().unwrap();
            assert_eq!(repos.len(), 1);
            // registered key is the canonical root; the worktree
            // path rides along for freshness checks
            assert_eq!(repos[0].path, canon_root);
            assert_eq!(
                repos[0].worktree_path(),
                Some(canon_root.join(discover::DIR))
            );
            // the whole repo vanishes — remove must still match
            std::fs::remove_dir_all(d.path().join("project")).unwrap();
            run_verb(
                "index.remove",
                &[d.path().join("project").to_string_lossy().into_owned()],
            )
            .unwrap();
            assert!(index.repos().unwrap().is_empty());
        });
    }

    /// A plain git repo — no `project/` worktree at all.
    fn repo_without_project() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        git(d.path(), &["init", "-q"]);
        git(d.path(), &["config", "user.email", "t@t"]);
        git(d.path(), &["config", "user.name", "t"]);
        d
    }

    #[test]
    fn add_registers_ledgerless_repo_without_snapshot() {
        let idx_dir = tempfile::tempdir().unwrap();
        let ds = idx_dir.path().to_str().unwrap().to_string();
        temp_env::with_var("MYCO_HOME", Some(ds.as_str()), || {
            let d = repo_without_project();
            run_verb("index.add", &[d.path().to_string_lossy().into_owned()]).unwrap();
            // the universe anchors at the process cwd — run from
            // inside a lasius workspace it would union every org
            // clone; the tempdir keeps this test registry-only
            let prev = std::env::current_dir().unwrap();
            std::env::set_current_dir(d.path()).unwrap();
            let index = RedbIndex::open().unwrap();
            let repos = index.repos().unwrap();
            assert_eq!(repos.len(), 1);
            // the explicit ledger-less state — no worktree, no
            // snapshot, basename as the name
            assert_eq!(repos[0].ledger, LedgerMode::None);
            assert_eq!(repos[0].worktree_path(), None);
            assert_eq!(
                repos[0].name,
                d.path().file_name().unwrap().to_string_lossy().into_owned()
            );
            assert!(index.snapshot(&repos[0].path).unwrap().is_none());
            // rebuild reports `no-ledger`, doesn't fail
            let out = run_verb("index.rebuild", &[]).unwrap();
            assert_eq!(out, 0);
            // org views omit the ledger-less repo entirely
            let views = crate::org::load_fresh(&index).unwrap();
            assert!(views.is_empty());
            std::env::set_current_dir(prev).unwrap();
        });
    }

    #[test]
    fn add_org_designates_single_ledger() {
        let idx_dir = tempfile::tempdir().unwrap();
        let ds = idx_dir.path().to_str().unwrap().to_string();
        temp_env::with_var("MYCO_HOME", Some(ds.as_str()), || {
            let a = repo_with_project();
            let b = repo_with_project();
            run_verb(
                "index.add",
                &[a.path().to_string_lossy().into_owned(), "--org".into()],
            )
            .unwrap();
            let index = RedbIndex::open().unwrap();
            let repos = index.repos().unwrap();
            assert_eq!(repos.len(), 1);
            assert_eq!(repos[0].ledger, LedgerMode::Org);
            // designating a second repo moves the mark — at most one
            run_verb(
                "index.add",
                &[b.path().to_string_lossy().into_owned(), "--org".into()],
            )
            .unwrap();
            let repos = index.repos().unwrap();
            assert_eq!(repos.len(), 2);
            let orgs: Vec<&RepoEntry> = repos
                .iter()
                .filter(|r| r.ledger == LedgerMode::Org)
                .collect();
            assert_eq!(orgs.len(), 1);
            assert_eq!(
                orgs[0].path,
                Utf8PathBuf::from_path_buf(b.path().canonicalize().unwrap()).unwrap()
            );
            // --org on a ledger-less repo refuses
            let c = repo_without_project();
            let e = run_verb(
                "index.add",
                &[c.path().to_string_lossy().into_owned(), "--org".into()],
            )
            .unwrap_err();
            assert!(e.contains("--org"), "{e}");
        });
    }

    #[test]
    fn add_preserves_org_designation_on_plain_readd() {
        let idx_dir = tempfile::tempdir().unwrap();
        let ds = idx_dir.path().to_str().unwrap().to_string();
        temp_env::with_var("MYCO_HOME", Some(ds.as_str()), || {
            let d = repo_with_project();
            let p = d.path().to_string_lossy().into_owned();
            run_verb("index.add", &[p.clone(), "--org".into()]).unwrap();
            // a plain re-add doesn't silently drop the org mark
            run_verb("index.add", &[p]).unwrap();
            let index = RedbIndex::open().unwrap();
            assert_eq!(index.repos().unwrap()[0].ledger, LedgerMode::Org);
        });
    }
}
