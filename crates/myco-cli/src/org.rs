//! `myco org status|board` — cross-repo views over the index.
//!
//! Every registered repo's snapshot is checked for freshness
//! (stored `head_commit` vs the worktree's current HEAD); a stale
//! or missing snapshot is rebuilt on the spot — org reads never
//! serve old data silently. Repos whose worktree vanished keep
//! their last snapshot and report an error inline.

use std::collections::BTreeMap;

use clap::Subcommand;
use myco_core::Status;
use myco_index::{Index, RepoSnapshot};
use serde_json::json;

use crate::{board, digest, indexcmd, ops};

#[derive(Subcommand)]
pub enum Cmd {
    /// Per-repo counts + org totals + items grouped by owner.
    Status,
    /// Org kanban — repo-prefixed cards over every live item.
    Board {
        /// Emit a markdown table (for docs/READMEs).
        #[arg(long)]
        md: bool,
    },
}

/// One repo's snapshot load outcome for org views.
pub struct RepoView {
    /// The snapshot (may be the last good one on rescan failure).
    pub snap: Option<RepoSnapshot>,
    /// Rescan/load error, when the fresh snapshot couldn't be had.
    pub error: Option<String>,
    /// The snapshot was rebuilt during this read.
    pub rebuilt: bool,
}

/// A scan-shaped adapter so `digest::counts` runs on snapshots.
fn as_scan(s: &RepoSnapshot) -> digest::Scan {
    digest::Scan {
        name: s.name.clone(),
        items: s.items.clone(),
        archived: s.archived.clone(),
    }
}

/// Load every registered repo's snapshot, auto-rebuilding stale
/// ones (head mismatch or missing).
pub fn load_fresh(index: &impl Index) -> Result<Vec<RepoView>, String> {
    let repos = index.repos().map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for r in repos {
        let root = r.path.as_std_path().to_path_buf();
        let head = indexcmd::current_head(&root);
        let cached = index.snapshot(&r.path).map_err(|e| e.to_string())?;
        let fresh = matches!((&cached, &head), (Some(s), Some(h)) if s.head_commit == *h);
        if fresh {
            out.push(RepoView {
                snap: cached,
                error: None,
                rebuilt: false,
            });
            continue;
        }
        match indexcmd::rebuild_one(index, &root) {
            Ok(s) => out.push(RepoView {
                snap: Some(s),
                error: None,
                rebuilt: true,
            }),
            Err(e) => out.push(RepoView {
                snap: cached,
                error: Some(e),
                rebuilt: false,
            }),
        }
    }
    Ok(out)
}

/// `owner → item refs` across all snapshots — active items only.
fn by_owner<'a>(snaps: impl Iterator<Item = &'a RepoSnapshot>) -> BTreeMap<String, Vec<String>> {
    let mut m: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for s in snaps {
        for i in s
            .live_items()
            .filter(|i| i.status != Status::Done && i.status != Status::Deferred)
        {
            if let Some(o) = &i.owner {
                m.entry(o.as_str().to_string())
                    .or_default()
                    .push(format!("{}:{} {}", s.name, i.id, i.title));
            }
        }
    }
    for v in m.values_mut() {
        v.sort();
    }
    m
}

fn status(views: &[RepoView], json_out: bool) -> Result<i32, String> {
    let mut total = digest::Counts::default();
    let mut repos_json = Vec::new();
    let mut snaps = Vec::new();
    for v in views {
        let Some(s) = &v.snap else {
            repos_json.push(json!({"error": v.error}));
            continue;
        };
        snaps.push(s);
        let c = digest::counts(&as_scan(s));
        total.add(&c);
        repos_json.push(json!({
            "name": s.name,
            "path": s.path,
            "head_commit": s.head_commit,
            "rebuilt": v.rebuilt,
            "error": v.error,
            "counts": c.to_json(),
        }));
    }
    let owners = by_owner(snaps.iter().copied());
    if json_out {
        ops::print_json(&json!({
            "repos": repos_json,
            "totals": total.to_json(),
            "owners": owners,
        }));
        return Ok(0);
    }
    println!("org status — {} repo(s)", views.len());
    for v in views {
        match &v.snap {
            Some(s) => {
                let c = digest::counts(&as_scan(s));
                let mut line = format!(
                    "{:<16} {}  @{}",
                    s.name,
                    c.oneline(),
                    &s.head_commit[..s.head_commit.len().min(7)],
                );
                if v.rebuilt {
                    line.push_str("  (rebuilt)");
                }
                if let Some(e) = &v.error {
                    line.push_str(&format!("  ⚠ {e}"));
                }
                println!("{line}");
            }
            None => println!(
                "{:<16} ⚠ {}",
                "?",
                v.error.as_deref().unwrap_or("unavailable")
            ),
        }
    }
    println!("totals: {}", total.oneline());
    if !owners.is_empty() {
        println!("by owner:");
        for (o, items) in &owners {
            println!("  {o}: {}", items.join(", "));
        }
    }
    Ok(0)
}

fn board_view(views: &[RepoView], md: bool, json_out: bool) -> Result<i32, String> {
    if json_out {
        // columns map with a `repo` tag on each item
        let mut merged = serde_json::Map::new();
        for (name, _) in board::COLUMNS {
            merged.insert(name.to_string(), json!([]));
        }
        for v in views {
            let Some(s) = &v.snap else { continue };
            let cols = board::columns_json(s.live_items(), Some(&s.name));
            for (name, _) in board::COLUMNS {
                let mut dst = merged[name].as_array().cloned().unwrap_or_default();
                dst.extend(cols[name].as_array().cloned().unwrap_or_default());
                merged.insert(name.to_string(), json!(dst));
            }
        }
        ops::print_json(&serde_json::Value::Object(merged));
        return Ok(0);
    }
    // human board: one merged column set, repo-prefixed cards
    let tagged = views.iter().flat_map(|v| {
        v.snap.iter().flat_map(|s| {
            s.live_items()
                .map(move |i| (format!("{}:{}", s.name, i.id), i))
        })
    });
    let cards = board::cards_by_column(tagged.map(|(id, i)| {
        let label = if i.title.is_empty() {
            id
        } else {
            format!("{id} {}", i.title)
        };
        (label, i)
    }));
    if cards.iter().all(Vec::is_empty) {
        println!("org board — nothing live across {} repo(s)", views.len());
    } else {
        println!("{}", board::render_cards(&cards, md));
    }
    Ok(0)
}

/// The `org` verb group.
pub fn run(cmd: Cmd, json_out: bool) -> Result<i32, String> {
    let index = indexcmd::open_index()?;
    let views = load_fresh(&index)?;
    match cmd {
        Cmd::Status => status(&views, json_out),
        Cmd::Board { md } => board_view(&views, md, json_out),
    }
}
