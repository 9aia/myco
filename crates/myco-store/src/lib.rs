//! Store port — where `project/` state is persisted. Narrow: load
//! and write each document shape atomically, allocate ids, list
//! item files per kind. Impls: `myco-store-md` (markdown worktree)
//! now, test doubles anywhere.
//!
//! The port talks in `myco-core` document models; the file grammar
//! belongs entirely to the impl.

use std::collections::BTreeSet;

use myco_core::{BacklogDoc, ItemFile, ItemId, ItemKind, ProjectMeta, TodoDoc};

/// Project-state persistence.
pub trait Store {
    /// `meta.toml` — load, or a default (`ProjectMeta::new`) when
    /// the file doesn't exist yet.
    fn meta(&self) -> Result<ProjectMeta, Error>;

    /// Write `meta.toml` atomically.
    fn save_meta(&self, meta: &ProjectMeta) -> Result<(), Error>;

    /// Allocate the next [`ItemId`] for `kind`: reconcile counters
    /// against ids already on disk (hand-edits can't reuse ids),
    /// bump, persist, return.
    fn alloc_id(&self, kind: ItemKind) -> Result<ItemId, Error>;

    /// Every item id currently in use — all kinds, all files.
    fn all_ids(&self) -> Result<BTreeSet<ItemId>, Error>;

    /// `TODO.md`.
    fn todo(&self) -> Result<TodoDoc, Error>;

    /// Write `TODO.md` atomically.
    fn save_todo(&self, doc: &TodoDoc) -> Result<(), Error>;

    /// `BACKLOG.md`.
    fn backlog(&self) -> Result<BacklogDoc, Error>;

    /// Write `BACKLOG.md` atomically.
    fn save_backlog(&self, doc: &BacklogDoc) -> Result<(), Error>;

    /// Load one item file by id (`I-003` → `issues/issue-0003-*.md`).
    /// Live files win over `archive/` copies; a same-id file in both
    /// is a duplicate-id error. [`Error::NotFound`] when absent.
    fn item(&self, id: ItemId) -> Result<ItemFile, Error>;

    /// Write an item file atomically. Filename derives from the
    /// meta (`<kind-file-prefix>-<NNNN>-<slug>.md`); when the title
    /// changed the slug, the old file is removed (rename).
    fn save_item(&self, file: &ItemFile) -> Result<(), Error>;

    /// Delete an item file — archive/remove verbs.
    /// [`Error::NotFound`] when absent.
    fn remove_item(&self, id: ItemId) -> Result<(), Error>;

    /// All item files of a kind, sorted by id. Task items aren't
    /// file-backed — [`ItemKind::Task`] yields an empty vec.
    /// Archived files (`<dir>/archive/`) are NOT included.
    fn list_items(&self, kind: ItemKind) -> Result<Vec<ItemFile>, Error>;

    /// Item files of a kind under `<dir>/archive/`, sorted by id —
    /// cold storage for done items (`myco issue archive`).
    fn list_archived(&self, kind: ItemKind) -> Result<Vec<ItemFile>, Error>;

    /// Move an item file into its kind's `archive/` subdir.
    /// [`Error::NotFound`] when there's no live file;
    /// [`Error::Invalid`] when it's already archived.
    fn archive_item(&self, id: ItemId) -> Result<(), Error>;

    /// Is `id`'s file under `<dir>/archive/`? Archived items stay
    /// addressable by [`Store::item`] but mutating verbs refuse
    /// them (they'd silently un-archive otherwise).
    fn is_archived(&self, id: ItemId) -> Result<bool, Error>;

    /// `index.md` verbatim — generated views land later; today it's
    /// a raw passthrough ([`Error::NotFound`] when absent).
    fn index_md(&self) -> Result<String, Error>;

    /// Write `index.md` atomically.
    fn save_index(&self, md: &str) -> Result<(), Error>;
}

/// Port error — impls map their richer errors into these variants;
/// `Parse` carries line/column so `myco check` can point.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Filesystem failure.
    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    /// The requested file doesn't exist.
    #[error("{path}: not found")]
    NotFound {
        /// The missing path (relative to the project root).
        path: String,
    },

    /// Malformed file — the impl couldn't parse it into the model.
    #[error("{path}:{line}:{column}: {message}")]
    Parse {
        /// Offending file (relative to the project root).
        path: String,
        /// 1-based line.
        line: usize,
        /// 1-based column.
        column: usize,
        /// What went wrong.
        message: String,
    },

    /// The model couldn't be written — e.g. a `done` item in
    /// `TODO.md`, a title containing `·`, a task passed to
    /// [`Store::item`].
    #[error("{0}")]
    Invalid(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display() {
        let e = Error::Parse {
            path: "TODO.md".into(),
            line: 4,
            column: 9,
            message: "bad status".into(),
        };
        assert_eq!(e.to_string(), "TODO.md:4:9: bad status");
        let io = Error::Io(std::io::Error::other("boom"));
        assert_eq!(io.to_string(), "io: boom");
        let nf = Error::NotFound {
            path: "meta.toml".into(),
        };
        assert_eq!(nf.to_string(), "meta.toml: not found");
        let inv = Error::Invalid("nope".into());
        assert_eq!(inv.to_string(), "nope");
    }
}
