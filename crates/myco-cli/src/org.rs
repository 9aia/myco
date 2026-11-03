//! `myco org` — cross-repo *scaffolding*; cross-repo *views* are
//! index-sourced `myco view` specs (`view org-board` /
//! `view org-status`, `--source index`).
//!
//! [`load_fresh`] is the shared index-universe read: every
//! registered repo's snapshot is checked for freshness (stored
//! `head_commit` vs the worktree's current HEAD); a stale or
//! missing snapshot is rebuilt on the spot — index reads never
//! serve old data silently. Repos whose worktree vanished keep
//! their last snapshot and report an error inline.

use myco_index::{Index, RepoSnapshot};
use serde_json::json;

use crate::registry::{Args, Ctx};
use crate::{indexcmd, ops};

/// One repo's snapshot load outcome for index-sourced views.
pub struct RepoView {
    /// Registry name — identity even when `snap` is gone.
    pub name: String,
    /// The snapshot (may be the last good one on rescan failure).
    pub snap: Option<RepoSnapshot>,
    /// Rescan/load error, when the fresh snapshot couldn't be had.
    pub error: Option<String>,
}

/// Load every registered repo's snapshot, auto-rebuilding stale
/// ones (head mismatch or missing). The per-repo `git`/fs work is
/// parallel — redb opens lock the file, so index I/O stays serial.
pub fn load_fresh(index: &impl Index) -> Result<Vec<RepoView>, String> {
    let repos = index.repos().map_err(|e| e.to_string())?;
    let heads = indexcmd::par_map(&repos, indexcmd::current_head);
    let mut cached = Vec::with_capacity(repos.len());
    for r in &repos {
        cached.push(index.snapshot(&r.path).map_err(|e| e.to_string())?);
    }
    let fresh =
        |i: usize| matches!((&cached[i], &heads[i]), (Some(s), Some(h)) if s.head_commit == *h);
    let stale: Vec<usize> = (0..repos.len()).filter(|i| !fresh(*i)).collect();
    let scans = indexcmd::par_map(&stale, |&i| indexcmd::scan_one(&repos[i]));
    let mut rebuilt: std::collections::BTreeMap<usize, Result<RepoSnapshot, String>> =
        stale.iter().copied().zip(scans).collect();
    let mut out = Vec::with_capacity(repos.len());
    for (i, c) in cached.iter().enumerate() {
        let name = repos[i].name.clone();
        if fresh(i) {
            out.push(RepoView {
                name,
                snap: c.clone(),
                error: None,
            });
            continue;
        }
        match rebuilt
            .remove(&i)
            .unwrap_or_else(|| Err("rescan lost".into()))
        {
            Ok(snap) => {
                index.save_snapshot(&snap).map_err(|e| e.to_string())?;
                out.push(RepoView {
                    name,
                    snap: Some(snap),
                    error: None,
                });
            }
            Err(e) => out.push(RepoView {
                name,
                snap: cached[i].clone(),
                error: Some(e),
            }),
        }
    }
    Ok(out)
}

/// `init` over every registered repo — per-repo outcomes, errors
/// collected (one bad repo doesn't stop the rollout).
fn init_all(index: &impl Index, json_out: bool, no_commit: bool) -> Result<i32, String> {
    let repos = index.repos().map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    let mut failures = 0usize;
    for r in &repos {
        let p = r.path.as_std_path().to_path_buf();
        match crate::init::ensure(None, Some(&p), no_commit) {
            Ok(o) => out.push(json!({
                "path": r.path,
                "files_created": o.files_created,
                "gitignore_updated": o.gitignore_updated,
                "agents_md_updated": o.agents_md_updated,
                "committed": o.committed,
            })),
            Err(e) => {
                failures += 1;
                out.push(json!({"path": r.path, "error": e}));
            }
        }
    }
    if json_out {
        ops::print_json(&json!({"repos": out, "failures": failures}));
        return Ok(if failures == 0 { 0 } else { 1 });
    }
    for (r, o) in repos.iter().zip(&out) {
        if let Some(e) = o.get("error").and_then(|v| v.as_str()) {
            println!("{:<24} ⚠ {e}", r.path.as_str());
        } else {
            let mut notes = Vec::new();
            if o["gitignore_updated"].as_bool().unwrap_or(false) {
                notes.push(".gitignore");
            }
            if o["agents_md_updated"].as_bool().unwrap_or(false) {
                notes.push("AGENTS.md");
            }
            let n = o["files_created"].as_array().map_or(0, Vec::len);
            if n > 0 {
                notes.push("scaffold");
            }
            println!(
                "{:<24} {}",
                r.path.as_str(),
                if notes.is_empty() {
                    "already current".to_string()
                } else {
                    format!("updated {}", notes.join(" + "))
                }
            );
        }
    }
    Ok(if failures == 0 { 0 } else { 1 })
}

/// The `org init` verb — rolls scaffold out (no snapshot reads).
pub fn h_init(ctx: &Ctx, _a: &Args) -> Result<i32, String> {
    let index = indexcmd::open_index()?;
    init_all(&index, ctx.json, ctx.no_commit)
}
