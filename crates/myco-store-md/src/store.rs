//! `MdStore` — the filesystem impl of [`myco_store::Store`] over a
//! `project/` worktree. Writes are atomic (tmp + rename).

use std::collections::BTreeSet;

use camino::{Utf8Path, Utf8PathBuf};
use myco_core::{BacklogDoc, ItemFile, ItemId, ItemKind, ProjectMeta, TodoDoc};
use myco_store::{Error, Store};

use crate::{backlog, itemfile, meta, todo};

/// The subdir of each item dir holding archived (done, shelved)
/// item files — `issues/archive/issue-NNNN-*.md`.
const ARCHIVE_DIR: &str = "archive";

/// A `project/` worktree on disk.
pub struct MdStore {
    root: Utf8PathBuf,
}

impl MdStore {
    /// Store rooted at the `project/` worktree dir.
    pub fn new(root: impl Into<Utf8PathBuf>) -> Self {
        MdStore { root: root.into() }
    }

    /// The worktree root.
    pub fn root(&self) -> &Utf8Path {
        &self.root
    }

    /// Repo name fallback — the worktree dir name (used when
    /// `meta.toml`/`TODO.md` don't exist yet).
    fn default_name(&self) -> String {
        self.root.file_name().unwrap_or("project").to_string()
    }

    fn path(&self, rel: &str) -> Utf8PathBuf {
        self.root.join(rel)
    }

    fn read(&self, rel: &str) -> Result<String, Error> {
        let p = self.path(rel);
        match std::fs::read_to_string(&p) {
            Ok(s) => Ok(s),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(Error::NotFound {
                path: rel.to_string(),
            }),
            Err(e) => Err(Error::Io(e)),
        }
    }

    /// Item files of a kind → `(id, path)` pairs, sorted by id.
    /// `archived` scopes the scan to `<dir>/archive/` — cold
    /// storage for done items. Non-matching names are skipped
    /// (`myco check` reports them).
    fn item_paths(
        &self,
        kind: ItemKind,
        archived: bool,
    ) -> Result<Vec<(ItemId, Utf8PathBuf)>, Error> {
        let Some(dir) = itemfile::item_dir(kind) else {
            return Ok(Vec::new());
        };
        let mut dir = self.path(dir);
        if archived {
            dir = dir.join(ARCHIVE_DIR);
        }
        let read = match std::fs::read_dir(&dir) {
            Ok(r) => r,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(Error::Io(e)),
        };
        let mut out = Vec::new();
        for entry in read {
            let entry = entry.map_err(Error::Io)?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            if let Some(id) = itemfile::parse_item_filename(kind, name) {
                // name.to_str() succeeded → dir.join(name) is utf8 too
                out.push((id, dir.join(name)));
            }
        }
        out.sort_by_key(|(id, _)| *id);
        Ok(out)
    }

    /// Parse every item file of a kind in one scope (live or
    /// `archive/`) — shared by `list_items`/`list_archived`.
    fn list_scoped(&self, kind: ItemKind, archived: bool) -> Result<Vec<ItemFile>, Error> {
        let mut out = Vec::new();
        for (_, path) in self.item_paths(kind, archived)? {
            let rel = path
                .strip_prefix(&self.root)
                .map_or_else(|_| path.as_str(), |r| r.as_str());
            let src = self.read(rel)?;
            out.push(itemfile::parse_item_file(&src, kind, rel)?);
        }
        Ok(out)
    }
}

impl Store for MdStore {
    fn meta(&self) -> Result<ProjectMeta, Error> {
        match self.read("meta.toml") {
            Ok(src) => Ok(meta::parse_meta(&src)?),
            Err(Error::NotFound { .. }) => Ok(ProjectMeta::new(self.default_name())),
            Err(e) => Err(e),
        }
    }

    fn save_meta(&self, m: &ProjectMeta) -> Result<(), Error> {
        write_atomic(&self.path("meta.toml"), &meta::emit_meta(m)?)
    }

    fn alloc_id(&self, kind: ItemKind) -> Result<ItemId, Error> {
        let mut meta = self.meta()?;
        let mut registry = meta.registry();
        registry.observe_all(self.all_ids()?);
        let id = registry.alloc(kind);
        meta.next = registry.counters();
        self.save_meta(&meta)?;
        Ok(id)
    }

    fn all_ids(&self) -> Result<BTreeSet<ItemId>, Error> {
        let mut ids = BTreeSet::new();
        if let Ok(doc) = self.todo() {
            ids.extend(doc.items().map(|i| i.id));
        }
        if let Ok(doc) = self.backlog() {
            ids.extend(doc.items().map(|i| i.id));
        }
        for kind in [
            ItemKind::Issue,
            ItemKind::Proposal,
            ItemKind::Adr,
            ItemKind::Plan,
        ] {
            // archived ids stay reserved — ids are never reused
            for archived in [false, true] {
                for (id, _) in self.item_paths(kind, archived)? {
                    ids.insert(id);
                }
            }
        }
        Ok(ids)
    }

    fn todo(&self) -> Result<TodoDoc, Error> {
        match self.read("TODO.md") {
            Ok(src) => Ok(todo::parse_todo(&src)?),
            Err(Error::NotFound { .. }) => Ok(TodoDoc::new(self.default_name())),
            Err(e) => Err(e),
        }
    }

    fn save_todo(&self, doc: &TodoDoc) -> Result<(), Error> {
        write_atomic(&self.path("TODO.md"), &todo::emit_todo(doc)?)
    }

    fn backlog(&self) -> Result<BacklogDoc, Error> {
        match self.read("BACKLOG.md") {
            Ok(src) => Ok(backlog::parse_backlog(&src)?),
            Err(Error::NotFound { .. }) => Ok(BacklogDoc::default()),
            Err(e) => Err(e),
        }
    }

    fn save_backlog(&self, doc: &BacklogDoc) -> Result<(), Error> {
        write_atomic(&self.path("BACKLOG.md"), &backlog::emit_backlog(doc)?)
    }

    fn item(&self, id: ItemId) -> Result<ItemFile, Error> {
        // live first, then archive — a same-id file in both scopes
        // surfaces as a duplicate rather than shadowing silently
        let mut hits = self
            .item_paths(id.kind(), false)?
            .into_iter()
            .chain(self.item_paths(id.kind(), true)?)
            .filter(|(i, _)| *i == id);
        let Some((_, path)) = hits.next() else {
            return Err(Error::NotFound {
                path: format!("{}/{id}", itemfile::item_dir(id.kind()).unwrap_or("<none>")),
            });
        };
        if hits.next().is_some() {
            return Err(Error::Invalid(format!("duplicate item files for {id}")));
        }
        let rel = path
            .strip_prefix(&self.root)
            .map_or_else(|_| path.as_str(), |r| r.as_str());
        let src = self.read(rel)?;
        Ok(itemfile::parse_item_file(&src, id.kind(), rel)?)
    }

    fn save_item(&self, file: &ItemFile) -> Result<(), Error> {
        let dir = itemfile::item_dir(file.meta.id.kind()).ok_or_else(|| {
            Error::Invalid("tasks aren't file items — they live in TODO.md/BACKLOG.md".into())
        })?;
        let filename = itemfile::item_filename(&file.meta.id, &file.meta.title)
            .ok_or_else(|| Error::Invalid("not a file-backed item kind".into()))?;
        let dir_path = self.path(dir);
        std::fs::create_dir_all(&dir_path).map_err(Error::Io)?;
        let target = dir_path.join(&filename);
        // rename: an existing file for the same id under a different
        // slug gets removed after the new one lands
        let old = self
            .item_paths(file.meta.id.kind(), false)?
            .into_iter()
            .filter(|(i, p)| *i == file.meta.id && *p != target)
            .map(|(_, p)| p)
            .next();
        write_atomic(&target, &itemfile::emit_item_file(file)?)?;
        if let Some(old) = old {
            std::fs::remove_file(&old).map_err(Error::Io)?;
        }
        Ok(())
    }

    fn remove_item(&self, id: ItemId) -> Result<(), Error> {
        let Some((_, path)) = self
            .item_paths(id.kind(), false)?
            .into_iter()
            .find(|(i, _)| *i == id)
        else {
            return Err(Error::NotFound {
                path: format!("{}/{id}", itemfile::item_dir(id.kind()).unwrap_or("<none>")),
            });
        };
        std::fs::remove_file(&path).map_err(Error::Io)
    }

    fn list_items(&self, kind: ItemKind) -> Result<Vec<ItemFile>, Error> {
        self.list_scoped(kind, false)
    }

    fn list_archived(&self, kind: ItemKind) -> Result<Vec<ItemFile>, Error> {
        self.list_scoped(kind, true)
    }

    fn archive_item(&self, id: ItemId) -> Result<(), Error> {
        let Some((_, path)) = self
            .item_paths(id.kind(), false)?
            .into_iter()
            .find(|(i, _)| *i == id)
        else {
            if self.is_archived(id)? {
                return Err(Error::Invalid(format!("{id} is already archived")));
            }
            return Err(Error::NotFound {
                path: format!("{}/{id}", itemfile::item_dir(id.kind()).unwrap_or("<none>")),
            });
        };
        let name = path.file_name().unwrap_or_default();
        let target = path
            .parent()
            .map(|p| p.join(ARCHIVE_DIR).join(name))
            .ok_or_else(|| Error::Invalid(format!("no parent dir for {path}")))?;
        if let Some(p) = target.parent() {
            std::fs::create_dir_all(p).map_err(Error::Io)?;
        }
        std::fs::rename(&path, &target).map_err(Error::Io)
    }

    fn is_archived(&self, id: ItemId) -> Result<bool, Error> {
        Ok(self
            .item_paths(id.kind(), true)?
            .into_iter()
            .any(|(i, _)| i == id))
    }

    fn index_md(&self) -> Result<String, Error> {
        self.read("index.md")
    }

    fn save_index(&self, md: &str) -> Result<(), Error> {
        write_atomic(&self.path("index.md"), md)
    }
}

/// Write `contents` to `path` atomically: same-dir tmp file +
/// `rename(2)`; fsync before the rename. Parent dirs are created.
pub fn write_atomic(path: &Utf8Path, contents: &str) -> Result<(), Error> {
    use std::io::Write;
    // bare filenames have an empty parent; "." mkdirs fine either way
    let parent = path
        .parent()
        .filter(|p| !p.as_str().is_empty())
        .unwrap_or_else(|| Utf8Path::new("."));
    std::fs::create_dir_all(parent).map_err(Error::Io)?;
    let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
    let write = (|| -> std::io::Result<()> {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(contents.as_bytes())?;
        f.sync_all()?;
        std::fs::rename(&tmp, path)
    })();
    if write.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    write.map_err(Error::Io)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use myco_core::{Item, Section, Status};

    fn store() -> (tempfile::TempDir, MdStore) {
        let dir = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(dir.path().join("proj")).unwrap();
        std::fs::create_dir_all(&root).unwrap();
        (dir, MdStore::new(root))
    }

    #[test]
    fn meta_default_then_save() {
        let (_d, s) = store();
        let m = s.meta().unwrap();
        assert_eq!(m.name, "proj"); // dir-name fallback
        assert_eq!(m.next.get(ItemKind::Task), 1);
        let mut m = m;
        m.name = "myco".into();
        s.save_meta(&m).unwrap();
        assert_eq!(s.meta().unwrap().name, "myco");
    }

    #[test]
    fn todo_roundtrip_and_default() {
        let (_d, s) = store();
        let mut doc = s.todo().unwrap();
        assert_eq!(doc.repo, "proj");
        assert!(doc.sections.is_empty());
        doc.sections.push(Section {
            name: Some("Now".into()),
            items: vec![Item::new(ItemId::new(ItemKind::Task, 1), "x")],
        });
        s.save_todo(&doc).unwrap();
        assert_eq!(s.todo().unwrap(), doc);
        assert!(s.root().join("TODO.md").exists());
    }

    #[test]
    fn backlog_default_and_save() {
        let (_d, s) = store();
        assert!(s.backlog().unwrap().sections.is_empty());
        let mut doc = BacklogDoc::default();
        doc.sections.push(Section {
            name: None,
            items: vec![Item::new(ItemId::new(ItemKind::Task, 3), "later")
                .status(Status::Deferred)],
        });
        s.save_backlog(&doc).unwrap();
        assert_eq!(s.backlog().unwrap(), doc);
    }

    #[test]
    fn item_lifecycle() {
        let (_d, s) = store();
        let file = ItemFile::new(Item::new(ItemId::new(ItemKind::Issue, 3), "Memory leak"))
            .body("## Description\n\nbody");
        s.save_item(&file).unwrap();
        assert!(s.root().join("issues/issue-0003-memory-leak.md").exists());
        assert_eq!(s.item(ItemId::new(ItemKind::Issue, 3)).unwrap(), file);

        // title change → slug change → rename
        let mut file2 = file.clone();
        file2.meta.title = "Leak fix".into();
        s.save_item(&file2).unwrap();
        assert!(s.root().join("issues/issue-0003-leak-fix.md").exists());
        assert!(!s.root().join("issues/issue-0003-memory-leak.md").exists());
        assert_eq!(s.list_items(ItemKind::Issue).unwrap().len(), 1);

        s.remove_item(ItemId::new(ItemKind::Issue, 3)).unwrap();
        assert!(matches!(
            s.item(ItemId::new(ItemKind::Issue, 3)),
            Err(Error::NotFound { .. })
        ));
        assert!(matches!(
            s.remove_item(ItemId::new(ItemKind::Issue, 3)),
            Err(Error::NotFound { .. })
        ));
    }

    #[test]
    fn archive_lifecycle() {
        let (_d, s) = store();
        let file =
            ItemFile::new(Item::new(ItemId::new(ItemKind::Issue, 3), "Leak").status(Status::Done));
        s.save_item(&file).unwrap();
        let live = s.root().join("issues/issue-0003-leak.md");
        assert!(live.exists());

        s.archive_item(ItemId::new(ItemKind::Issue, 3)).unwrap();
        assert!(!live.exists());
        let archived = s.root().join("issues/archive/issue-0003-leak.md");
        assert!(archived.exists());

        // excluded from live lists, present in archived + item lookup
        assert!(s.list_items(ItemKind::Issue).unwrap().is_empty());
        assert_eq!(s.list_archived(ItemKind::Issue).unwrap().len(), 1);
        assert_eq!(s.item(ItemId::new(ItemKind::Issue, 3)).unwrap(), file);
        assert!(s.is_archived(ItemId::new(ItemKind::Issue, 3)).unwrap());
        assert!(!s.is_archived(ItemId::new(ItemKind::Issue, 9)).unwrap());
        // the archived id still feeds the registry — never reused
        assert!(s
            .all_ids()
            .unwrap()
            .contains(&ItemId::new(ItemKind::Issue, 3)));

        // already-archived and missing both error
        assert!(matches!(
            s.archive_item(ItemId::new(ItemKind::Issue, 3)),
            Err(Error::Invalid(_))
        ));
        assert!(matches!(
            s.archive_item(ItemId::new(ItemKind::Issue, 9)),
            Err(Error::NotFound { .. })
        ));
        assert!(matches!(
            s.archive_item(ItemId::new(ItemKind::Task, 1)),
            Err(Error::NotFound { .. })
        ));

        // live + archive same-id → duplicate error, not a silent shadow
        s.save_item(&file).unwrap();
        assert!(matches!(
            s.item(ItemId::new(ItemKind::Issue, 3)),
            Err(Error::Invalid(_))
        ));
    }

    #[test]
    fn item_rejects_tasks_and_bad_kind() {
        let (_d, s) = store();
        let t = ItemFile::new(Item::new(ItemId::new(ItemKind::Task, 1), "x"));
        assert!(s.save_item(&t).is_err());
        assert!(s.item(ItemId::new(ItemKind::Task, 1)).is_err());
        assert!(s.list_items(ItemKind::Task).unwrap().is_empty());
    }

    #[test]
    fn alloc_reconciles_against_files() {
        let (_d, s) = store();
        // hand-placed items + stale meta
        std::fs::create_dir_all(s.root().join("issues")).unwrap();
        std::fs::write(
            s.root().join("issues/issue-0007-x.md"),
            "+++\nid = \"I-007\"\ntitle = \"x\"\nstatus = \"open\"\n+++\n",
        )
        .unwrap();
        let mut doc = TodoDoc::new("proj");
        doc.sections.push(Section {
            name: None,
            items: vec![Item::new(ItemId::new(ItemKind::Task, 40), "old")],
        });
        s.save_todo(&doc).unwrap();
        assert_eq!(
            s.alloc_id(ItemKind::Task).unwrap(),
            ItemId::new(ItemKind::Task, 41)
        );
        assert_eq!(
            s.alloc_id(ItemKind::Issue).unwrap(),
            ItemId::new(ItemKind::Issue, 8)
        );
        assert_eq!(
            s.alloc_id(ItemKind::Plan).unwrap(),
            ItemId::new(ItemKind::Plan, 1)
        );
        // persisted
        assert_eq!(s.meta().unwrap().next.get(ItemKind::Task), 42);
    }

    #[test]
    fn all_ids_collects() {
        let (_d, s) = store();
        let mut doc = TodoDoc::new("proj");
        doc.sections.push(Section {
            name: None,
            items: vec![Item::new(ItemId::new(ItemKind::Task, 5), "x")],
        });
        s.save_todo(&doc).unwrap();
        s.save_item(&ItemFile::new(Item::new(
            ItemId::new(ItemKind::Proposal, 2),
            "p",
        )))
        .unwrap();
        let ids = s.all_ids().unwrap();
        assert!(ids.contains(&ItemId::new(ItemKind::Task, 5)));
        assert!(ids.contains(&ItemId::new(ItemKind::Proposal, 2)));
    }

    #[test]
    fn index_passthrough() {
        let (_d, s) = store();
        assert!(matches!(s.index_md(), Err(Error::NotFound { .. })));
        s.save_index("# Index\n\n- links\n").unwrap();
        assert_eq!(s.index_md().unwrap(), "# Index\n\n- links\n");
    }

    #[test]
    fn atomic_write_creates_dirs_and_leaves_no_tmp() {
        let dir = tempfile::tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(dir.path().join("sub/deep/f.md")).unwrap();
        write_atomic(&path, "hi").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "hi");
        // no tmp litter
        let entries: Vec<_> = std::fs::read_dir(path.parent().unwrap()).unwrap().collect();
        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn unreadable_file_is_io_error_not_notfound() {
        use std::os::unix::fs::PermissionsExt;
        let (_d, s) = store();
        for f in ["TODO.md", "meta.toml", "BACKLOG.md"] {
            let p = s.root().join(f);
            std::fs::write(&p, "x").unwrap();
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o000)).unwrap();
        }
        assert!(matches!(s.todo(), Err(Error::Io(_))));
        assert!(matches!(s.meta(), Err(Error::Io(_))));
        assert!(matches!(s.backlog(), Err(Error::Io(_))));
    }

    #[test]
    fn malformed_files_propagate_parse_errors() {
        let (_d, s) = store();
        std::fs::write(s.root().join("TODO.md"), "nope\n").unwrap();
        std::fs::write(s.root().join("BACKLOG.md"), "nope\n").unwrap();
        std::fs::write(s.root().join("meta.toml"), "nope = [").unwrap();
        assert!(matches!(s.todo(), Err(Error::Parse { .. })));
        assert!(matches!(s.backlog(), Err(Error::Parse { .. })));
        assert!(matches!(s.meta(), Err(Error::Parse { .. })));
        // all_ids skips the broken docs, keeps file-backed ids
        s.save_item(&ItemFile::new(Item::new(
            ItemId::new(ItemKind::Issue, 1),
            "ok",
        )))
        .unwrap();
        let ids = s.all_ids().unwrap();
        assert_eq!(ids.len(), 1);
    }

    #[test]
    fn item_paths_edge_cases() {
        use std::os::unix::ffi::OsStrExt;
        let (_d, s) = store();
        // `issues` as a regular file → read_dir fails (not NotFound)
        std::fs::write(s.root().join("issues"), "x").unwrap();
        assert!(matches!(s.list_items(ItemKind::Issue), Err(Error::Io(_))));
        // non-UTF8 filename is skipped
        std::fs::remove_file(s.root().join("issues")).unwrap();
        let dir = s.root().join("issues");
        std::fs::create_dir_all(&dir).unwrap();
        let weird = dir
            .as_std_path()
            .join(std::ffi::OsStr::from_bytes(b"\xff.md"));
        std::fs::write(&weird, "x").unwrap();
        assert!(s.list_items(ItemKind::Issue).unwrap().is_empty());
    }

    #[test]
    fn duplicate_item_files_error() {
        let (_d, s) = store();
        let f = ItemFile::new(Item::new(ItemId::new(ItemKind::Issue, 3), "a"));
        s.save_item(&f).unwrap();
        let mut f2 = f.clone();
        f2.meta.title = "b".into();
        // same id, different slug → two files for I-003
        std::fs::write(
            s.root().join("issues/issue-0003-other.md"),
            crate::itemfile::emit_item_file(&f2).unwrap(),
        )
        .unwrap();
        assert!(matches!(
            s.item(ItemId::new(ItemKind::Issue, 3)),
            Err(Error::Invalid(_))
        ));
    }

    #[test]
    fn write_atomic_cleans_tmp_on_failure() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let sub = Utf8PathBuf::from_path_buf(dir.path().join("ro")).unwrap();
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::set_permissions(sub.as_std_path(), std::fs::Permissions::from_mode(0o555))
            .unwrap();
        let target = sub.join("f.md");
        assert!(write_atomic(&target, "hi").is_err());
        // tmp file cleaned up
        let left: Vec<_> = std::fs::read_dir(&sub).unwrap().collect();
        assert!(left.is_empty());
        std::fs::set_permissions(sub.as_std_path(), std::fs::Permissions::from_mode(0o755))
            .unwrap();
    }
}
