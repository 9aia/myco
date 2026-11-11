//! Undo journal — `myco-history.json` in the repo's git common
//! dir (`<main>/.git`, shared by every worktree). It records only
//! the *future*: commits `undo` moved away from that `redo` can
//! still restore. The ledger's linear commit chain stays the
//! "past" — the journal is disposable derived state, never
//! committed to `project/`.
//!
//! Each entry anchors on `base`: the HEAD position it was undone
//! *from*. Redo fires only while HEAD still equals the top
//! entry's `base` — any commit, pull, or manual git op breaks the
//! anchor and the stack self-clears on next read. No hooks into
//! commit paths; the HEAD check *is* the invalidation.
//!
//! Robustness: a missing or corrupt file reads as empty history
//! (redo refuses, undo rewrites it); saves are tmp+rename so a
//! crash mid-write can't leave a torn journal; entries are
//! shape-validated on load and the stack is capped.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Journal file name inside the git common dir.
const FILE: &str = "myco-history.json";
/// Schema version — unknown versions read as empty.
const VERSION: u32 = 1;
/// Bound the stack — pathological undo×N drops the oldest
/// (least-recently-undone) entries first.
const MAX_ENTRIES: usize = 100;

/// One redoable commit — `target` is what `git reset --hard`
/// restores, `base` the HEAD it was undone from.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    /// Commit hash redo resets to (40-hex, validated on load).
    pub target: String,
    /// HEAD sha the entry was undone from — the validity anchor.
    pub base: String,
    /// Subject line, captured at undo time for display.
    pub subject: String,
    /// Author date (`%as`), same source as [`crate::ops::UndoOutcome`].
    pub date: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Journal {
    version: u32,
    /// Undone commits, oldest → newest; the *top* (last) is the
    /// next redo target.
    pub(crate) future: Vec<HistoryEntry>,
}

impl Default for Journal {
    fn default() -> Self {
        Self {
            version: VERSION,
            future: Vec::new(),
        }
    }
}

/// `<git-common-dir>/myco-history.json` — `None` outside a git
/// repo (callers treat that as "no history", same as empty).
fn path(dir: &Path) -> Option<PathBuf> {
    crate::vcs::common_dir(dir).ok().map(|d| d.join(FILE))
}

fn is_sha(s: &str) -> bool {
    s.len() == 40 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Read the journal. Missing file, bad JSON, wrong version, or
/// malformed entries all degrade to empty history — the journal
/// is derived state and must never break a verb.
pub(crate) fn load(dir: &Path) -> Journal {
    let Some(p) = path(dir) else {
        return Journal::default();
    };
    let Ok(raw) = std::fs::read_to_string(&p) else {
        return Journal::default();
    };
    let Ok(mut j) = serde_json::from_str::<Journal>(&raw) else {
        return Journal::default();
    };
    if j.version != VERSION {
        return Journal::default();
    }
    j.future.retain(|e| is_sha(&e.target) && is_sha(&e.base));
    Journal {
        version: VERSION,
        future: j.future,
    }
}

/// Atomic write — tmp file in the same dir + rename, fsynced, so
/// a crash can't leave a torn journal. Failure is the caller's
/// problem (undo/redo degrade to a warning, never a hard error).
pub(crate) fn save(dir: &Path, j: &Journal) -> Result<(), String> {
    let Some(p) = path(dir) else {
        return Err("not a git repository".into());
    };
    let tmp = p.with_extension("json.tmp");
    let body = serde_json::to_string(j).map_err(|e| e.to_string())?;
    {
        use std::io::Write;
        let mut f = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
        f.write_all(body.as_bytes()).map_err(|e| e.to_string())?;
        f.sync_all().map_err(|e| e.to_string())?;
    }
    std::fs::rename(&tmp, &p).map_err(|e| e.to_string())?;
    // fsync the dir so the rename itself is durable.
    if let Ok(d) = std::fs::File::open(p.parent().unwrap_or(&p)) {
        let _ = d.sync_all();
    }
    Ok(())
}

/// `HEAD`'s sha — the anchor every validity check compares against.
pub fn head_sha(dir: &Path) -> Result<String, String> {
    crate::vcs::git_out(dir, &["rev-parse", "HEAD"]).map_err(|e| e.to_string())
}

/// Is the stack's top still anchored? `anchor` is the HEAD the
/// caller just observed (pre-reset for undo, current for redo).
pub(crate) fn top_valid(j: &Journal, anchor: &str) -> bool {
    j.future.last().is_some_and(|e| e.base == anchor)
}

/// Drop the whole stack — the timeline moved underneath it.
pub(crate) fn clear(j: &mut Journal) {
    j.future.clear();
}

/// Push an undone commit; drops the oldest entries past the cap.
pub(crate) fn push(j: &mut Journal, e: HistoryEntry) {
    j.future.push(e);
    let excess = j.future.len().saturating_sub(MAX_ENTRIES);
    if excess > 0 {
        j.future.drain(..excess);
    }
}

/// How many commits redo could still restore — 0 when the top
/// anchor doesn't match `head` (stale stack reads as empty).
pub fn depth(dir: &Path, head: &str) -> usize {
    let j = load(dir);
    if top_valid(&j, head) {
        j.future.len()
    } else {
        0
    }
}

/// The redoable stack, top-first (next redo first) — for
/// `myco undo --list`.
pub fn list(dir: &Path) -> Vec<HistoryEntry> {
    let j = load(dir);
    j.future.iter().rev().cloned().collect()
}
