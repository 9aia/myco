//! Index port — the derived, disposable cache that powers
//! index-sourced `myco view` (the `org-*` presets).
//!
//! Markdown in `project/` stays canonical; a [`RepoSnapshot`] is a
//! frozen scan of one repo's ledger keyed by the project-branch
//! HEAD commit it was taken at. Freshness is a one-line check —
//! `snapshot.head_commit == git rev-parse HEAD` — so callers can
//! rebuild exactly the stale repos. Deleting the whole index is
//! always safe: `myco index rebuild` derives it back.
//!
//! Impls: `myco-index-redb` (`~/.local/share/myco/`).
//!
//! The second face is the link index ([`links`], P-005):
//! `project/` files → `(src, dst, kind)` edges, invalidated by
//! content fingerprint — same "derived, disposable" contract.

pub mod links;

use std::collections::BTreeSet;

use camino::{Utf8Path, Utf8PathBuf};
use myco_core::{Item, ItemId};

pub use links::{fingerprint, FileStamp, LinkIndex, Related};

/// A registered repo's ledger relationship — the `ledger` key in
/// `repos.toml` or `[index] ledger` in `myco.toml`. The type lives
/// in `myco-core` (the config model names it); re-exported here so
/// `myco_index::LedgerMode` paths keep working. Absent →
/// [`LedgerMode::Local`], the conventional reading.
pub use myco_core::LedgerMode;

/// A registered repo — an entry in the index's registry.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RepoEntry {
    /// Canonical repo-root path — the registry key.
    pub path: Utf8PathBuf,
    /// Display name (project `meta.toml` name at registration).
    pub name: String,
    /// The `project/` worktree the index scans — set at
    /// registration so `--dir`/`project_dir` worktrees (not at
    /// `<path>/project`) stay reachable. Empty means the
    /// conventional location (registries written before this
    /// field existed).
    #[serde(default)]
    pub worktree: Utf8PathBuf,
    /// Ledger relationship — `local` (own ledger), `org` (the
    /// org/global ledger), `none` (explicitly ledger-less).
    #[serde(default, skip_serializing_if = "LedgerMode::is_local")]
    pub ledger: LedgerMode,
}

impl RepoEntry {
    /// New registry entry — `local` ledger, conventional worktree.
    pub fn new(path: Utf8PathBuf, name: impl Into<String>) -> Self {
        RepoEntry {
            path,
            name: name.into(),
            worktree: Utf8PathBuf::new(),
            ledger: LedgerMode::Local,
        }
    }

    /// Record the scanned worktree path.
    pub fn with_worktree(mut self, worktree: Utf8PathBuf) -> Self {
        self.worktree = worktree;
        self
    }

    /// Mark the ledger relationship (`org` / `none`).
    pub fn with_ledger(mut self, ledger: LedgerMode) -> Self {
        self.ledger = ledger;
        self
    }

    /// `false` for explicitly ledger-less (`ledger = "none"`)
    /// repos — they hold no snapshot and contribute no items.
    pub fn has_ledger(&self) -> bool {
        self.ledger != LedgerMode::None
    }

    /// The worktree to scan — the registered `worktree`, else the
    /// conventional `<path>/project`; `None` for ledger-less
    /// repos.
    pub fn worktree_path(&self) -> Option<Utf8PathBuf> {
        if self.ledger == LedgerMode::None {
            return None;
        }
        Some(if self.worktree.as_str().is_empty() {
            self.path.join("project")
        } else {
            self.worktree.clone()
        })
    }
}

/// A frozen scan of one repo's `project/` ledger.
///
/// `items` holds every item found — TODO/BACKLOG lines and
/// file-backed metas, live and archived. `archived` marks which ids
/// live under `<dir>/archive/` so views can split "resolved" from
/// "live".
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RepoSnapshot {
    /// Canonical repo-root path — the snapshot key.
    pub path: Utf8PathBuf,
    /// Project name (from `meta.toml` / `TODO.md`).
    pub name: String,
    /// The project-worktree `HEAD` this scan was taken at —
    /// freshness is `head_commit == current HEAD`.
    pub head_commit: String,
    /// RFC 3339 timestamp of the scan.
    pub scanned_at: String,
    /// Every item in the ledger (any status, live + archived).
    pub items: Vec<Item>,
    /// Ids whose files live under `archive/`.
    #[serde(default)]
    pub archived: BTreeSet<ItemId>,
}

impl RepoSnapshot {
    /// Items not archived — the live ledger. Nested children ride
    /// inside their parents in `items`; they flatten out here so
    /// org views count them like any item.
    pub fn live_items(&self) -> impl Iterator<Item = &Item> {
        self.items
            .iter()
            .flat_map(Item::subtree)
            .filter(|i| !self.archived.contains(&i.id))
    }
}

/// The index port — registry + snapshot store.
pub trait Index {
    /// Register (or re-register) a repo root.
    fn register(&self, repo: &RepoEntry) -> Result<(), Error>;

    /// Unregister a repo and drop its snapshot. `Ok(true)` when it
    /// was registered.
    fn remove(&self, path: &Utf8Path) -> Result<bool, Error>;

    /// Every registered repo, sorted by path.
    fn repos(&self) -> Result<Vec<RepoEntry>, Error>;

    /// The cached snapshot for `path`, `None` when absent.
    fn snapshot(&self, path: &Utf8Path) -> Result<Option<RepoSnapshot>, Error>;

    /// Write (or overwrite) a repo's snapshot.
    fn save_snapshot(&self, snapshot: &RepoSnapshot) -> Result<(), Error>;
}

/// Port error — impls map backend failures into these variants.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Filesystem failure (registry file, db path).
    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    /// The snapshot couldn't be (de)serialized.
    #[error("snapshot codec: {0}")]
    Codec(String),

    /// Backend failure (redb, …).
    #[error("index: {0}")]
    Backend(String),
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use myco_core::{ItemId, ItemKind, Status};

    #[test]
    fn entry_and_snapshot_serde() {
        let e = RepoEntry::new(Utf8PathBuf::from("/repo"), "myco");
        let snap = RepoSnapshot {
            path: e.path.clone(),
            name: "myco".into(),
            head_commit: "deadbeef".into(),
            scanned_at: "2026-02-09T00:00:00Z".into(),
            items: vec![
                Item::new(ItemId::new(ItemKind::Task, 1), "open"),
                Item::new(ItemId::new(ItemKind::Issue, 2), "done").status(Status::Done),
                Item::new(ItemId::new(ItemKind::Issue, 3), "arch"),
            ],
            archived: BTreeSet::from([ItemId::new(ItemKind::Issue, 3)]),
        };
        let json = serde_json::to_string(&snap).unwrap();
        let back: RepoSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(back, snap);
        // live = non-archived (done still counts — views filter it)
        let live: Vec<ItemId> = back.live_items().map(|i| i.id).collect();
        assert_eq!(
            live,
            [
                ItemId::new(ItemKind::Task, 1),
                ItemId::new(ItemKind::Issue, 2)
            ]
        );
    }

    #[test]
    fn entry_ledger_serde_backcompat() {
        // registries written before `ledger` existed deserialize
        // as `local` — the conventional `<path>/project` reading
        let e: RepoEntry =
            serde_json::from_str(r#"{"path":"/r","name":"r","worktree":""}"#).unwrap();
        assert_eq!(e.ledger, LedgerMode::Local);
        assert_eq!(e.worktree_path(), Some(Utf8PathBuf::from("/r/project")));
        // `none`/`org` round-trip; `local` stays out of the file
        let none = RepoEntry::new(Utf8PathBuf::from("/r"), "r").with_ledger(LedgerMode::None);
        assert_eq!(none.worktree_path(), None);
        assert!(!none.has_ledger());
        let org = RepoEntry::new(Utf8PathBuf::from("/o"), "o")
            .with_worktree(Utf8PathBuf::from("/o/project"))
            .with_ledger(LedgerMode::Org);
        let json = serde_json::to_string(&org).unwrap();
        assert!(json.contains("\"org\""), "{json}");
        let back: RepoEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(back, org);
        // a local entry writes no `ledger` key at all
        let local = RepoEntry::new(Utf8PathBuf::from("/l"), "l");
        assert!(!serde_json::to_string(&local).unwrap().contains("ledger"));
    }

    #[test]
    fn error_display() {
        assert_eq!(Error::Codec("x".into()).to_string(), "snapshot codec: x");
        assert_eq!(Error::Backend("y".into()).to_string(), "index: y");
    }
}
