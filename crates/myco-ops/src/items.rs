//! Item ops — the I/O shell around `myco-core`'s domain: store
//! open/save, the commit-per-mutation tail, item-file reads, and
//! the shared output verbs. The pure pieces (ledger doc surgery,
//! state walks, `compact`, `tree_view`, `require_kind`,
//! `NewItemMeta`) live in `myco-core` and are re-exported here so
//! call sites read `items::X` either way.

use std::path::Path;

use myco_core::{Item, ItemFile, ItemId, ItemKind, Status};
use myco_store::Store;
use myco_store_md::MdStore;
use serde_json::json;

use crate::discover;
use crate::ops;

pub use myco_core::{
    require_kind, status_path, targets, tree_view, walk_descendants_to, walk_to, Ledger,
    NewItemMeta, TaskLoc,
};

/// `T-001 open pri:high owner:devin #core title` — the compact
/// human list line. Thin wrapper over [`Item::compact`].
pub fn compact(item: &Item) -> String {
    item.compact()
}

/// Load both ledger docs through the store.
pub fn load_ledger(store: &MdStore) -> Result<Ledger, String> {
    Ok(Ledger::new(
        store.todo().map_err(|e| e.to_string())?,
        store.backlog().map_err(|e| e.to_string())?,
    ))
}

/// Write both ledger docs back through the store.
pub fn save_ledger(store: &MdStore, ledger: &Ledger) -> Result<(), String> {
    store.save_todo(&ledger.todo).map_err(|e| e.to_string())?;
    store
        .save_backlog(&ledger.backlog)
        .map_err(|e| e.to_string())
}

/// Load a file-backed item for mutation. Archived items are
/// refused — `save_item` writes to the live dir, so editing one
/// would silently un-archive it.
pub fn item_for_edit(store: &MdStore, id: ItemId) -> Result<ItemFile, String> {
    if store.is_archived(id).map_err(|e| e.to_string())? {
        return Err(format!("{id} is archived — mutating it is refused"));
    }
    store.item(id).map_err(|e| e.to_string())
}

/// Today's date — `created:` values and comment timestamps.
pub fn today() -> time::Date {
    time::OffsetDateTime::now_utc().date()
}

/// A store handle holding the worktree lock — exclusive from
/// [`open`] (mutating verbs), shared from [`open_read`]. The lock
/// releases when this drops. Derefs to [`MdStore`], so `&locked`
/// coerces to `&MdStore` at every call site.
pub struct LockedStore {
    store: MdStore,
    _lock: Option<ops::WorktreeLock>,
}

impl LockedStore {
    /// A store holding no lock — for tests and callers that
    /// guarantee a single writer by construction.
    pub fn unlocked(store: MdStore) -> Self {
        LockedStore { store, _lock: None }
    }
}

impl std::ops::Deref for LockedStore {
    type Target = MdStore;

    fn deref(&self) -> &MdStore {
        &self.store
    }
}

/// `LockedStore` is a full [`Store`] — pure delegation, so
/// `&locked` satisfies `&impl Store` bounds (generic params don't
/// deref-coerce).
impl Store for LockedStore {
    fn meta(&self) -> Result<myco_core::ProjectMeta, myco_store::Error> {
        self.store.meta()
    }
    fn save_meta(&self, m: &myco_core::ProjectMeta) -> Result<(), myco_store::Error> {
        self.store.save_meta(m)
    }
    fn alloc_id(&self, kind: ItemKind) -> Result<ItemId, myco_store::Error> {
        self.store.alloc_id(kind)
    }
    fn all_ids(&self) -> Result<std::collections::BTreeSet<ItemId>, myco_store::Error> {
        self.store.all_ids()
    }
    fn todo(&self) -> Result<myco_core::TodoDoc, myco_store::Error> {
        self.store.todo()
    }
    fn save_todo(&self, doc: &myco_core::TodoDoc) -> Result<(), myco_store::Error> {
        self.store.save_todo(doc)
    }
    fn backlog(&self) -> Result<myco_core::BacklogDoc, myco_store::Error> {
        self.store.backlog()
    }
    fn save_backlog(&self, doc: &myco_core::BacklogDoc) -> Result<(), myco_store::Error> {
        self.store.save_backlog(doc)
    }
    fn item(&self, id: ItemId) -> Result<ItemFile, myco_store::Error> {
        self.store.item(id)
    }
    fn save_item(&self, file: &ItemFile) -> Result<(), myco_store::Error> {
        self.store.save_item(file)
    }
    fn remove_item(&self, id: ItemId) -> Result<(), myco_store::Error> {
        self.store.remove_item(id)
    }
    fn list_items(&self, kind: ItemKind) -> Result<Vec<ItemFile>, myco_store::Error> {
        self.store.list_items(kind)
    }
    fn list_archived(&self, kind: ItemKind) -> Result<Vec<ItemFile>, myco_store::Error> {
        self.store.list_archived(kind)
    }
    fn archive_item(&self, id: ItemId) -> Result<(), myco_store::Error> {
        self.store.archive_item(id)
    }
    fn is_archived(&self, id: ItemId) -> Result<bool, myco_store::Error> {
        self.store.is_archived(id)
    }
    fn index_md(&self) -> Result<String, myco_store::Error> {
        self.store.index_md()
    }
    fn save_index(&self, md: &str) -> Result<(), myco_store::Error> {
        self.store.save_index(md)
    }
    fn read_handoffs(&self, id: ItemId) -> Result<Vec<myco_core::HandoffEntry>, myco_store::Error> {
        self.store.read_handoffs(id)
    }
    fn append_handoff(
        &self,
        id: ItemId,
        entry: &myco_core::HandoffEntry,
    ) -> Result<(), myco_store::Error> {
        self.store.append_handoff(id, entry)
    }
    fn list_handoff_files(&self) -> Result<Vec<myco_store::HandoffFile>, myco_store::Error> {
        self.store.list_handoff_files()
    }
}

/// Resolve the project + open the store under the exclusive
/// mutation lock — every verb that writes the ledger takes this
/// path. The lock spans discovery → mutation → the per-verb
/// autocommit; contention fails fast (`another myco process is
/// writing — retry`) instead of corrupting the ledger.
pub fn open(
    dir: Option<&Path>,
    root: Option<&Path>,
) -> Result<(discover::Project, LockedStore), String> {
    let proj = discover::discover(dir, root)?;
    let lock = ops::lock_worktree(&proj.worktree)?;
    let store = ops::store_at(&proj.worktree)?;
    Ok((
        proj,
        LockedStore {
            store,
            _lock: Some(lock),
        },
    ))
}

/// Resolve + open under the shared read lock — read-only verbs
/// (`status`, `board`, `list`, `resume`, …). Readers don't block
/// each other; a reader meeting a live writer fails fast.
pub fn open_read(
    dir: Option<&Path>,
    root: Option<&Path>,
) -> Result<(discover::Project, LockedStore), String> {
    let proj = discover::discover(dir, root)?;
    let lock = ops::lock_shared(&proj.worktree)?;
    let store = ops::store_at(&proj.worktree)?;
    Ok((
        proj,
        LockedStore {
            store,
            _lock: Some(lock),
        },
    ))
}

/// The standard mutation tail — one atomic commit on the worktree.
pub fn commit(wt: &Path, msg: &str, no_commit: bool) -> Result<bool, String> {
    ops::autocommit(wt, msg, ops::commits_enabled(no_commit))
}

/// Standard mutation output: `--json` gets `payload` plus a
/// `committed` key; human output is one line.
pub fn report(
    mut payload: serde_json::Value,
    committed: bool,
    json_out: bool,
    human: &str,
) -> Result<i32, String> {
    if json_out {
        payload["committed"] = json!(committed);
        ops::print_json(&payload);
    } else {
        println!("{human}");
    }
    Ok(0)
}

/// `--status/--owner/--label` list filter + `--limit` — the parsed
/// form of the CLI's `ListFlags`, usable by any front-end.
#[derive(Debug, Clone, Default)]
pub struct Filter {
    /// Keep only items with this status.
    pub status: Option<Status>,
    /// Keep only items owned by this actor string.
    pub owner: Option<String>,
    /// Keep only items carrying all of these labels.
    pub labels: Vec<String>,
    /// Keep only the first N items after filtering.
    pub limit: Option<usize>,
}

impl Filter {
    /// Does `item` pass the filters?
    pub fn matches(&self, item: &Item) -> bool {
        self.status.is_none_or(|s| item.status == s)
            && self
                .owner
                .as_deref()
                .is_none_or(|o| item.owner.as_ref().map(myco_core::Actor::as_str) == Some(o))
            && self.labels.iter().all(|l| item.labels.contains(l))
    }

    /// `limit` as a `.take()` bound.
    pub fn take(&self) -> usize {
        self.limit.unwrap_or(usize::MAX)
    }
}

/// `--json` → a bare array of item objects; else compact lines.
pub fn print_list(items: &[Item], json_out: bool) -> Result<i32, String> {
    if json_out {
        let v: Vec<serde_json::Value> = items
            .iter()
            .map(|i| serde_json::to_value(i).unwrap_or(json!(null)))
            .collect();
        ops::print_json(&json!(v));
    } else {
        for i in items {
            println!("{}", compact(i));
        }
    }
    Ok(0)
}

/// Like [`print_list`] for depth-tagged items — human output
/// indents children 2 spaces per level under their parent.
pub fn print_tree(pairs: &[(usize, &Item)], json_out: bool) -> Result<i32, String> {
    if json_out {
        let v: Vec<serde_json::Value> = pairs
            .iter()
            .map(|(_, i)| serde_json::to_value(i).unwrap_or(json!(null)))
            .collect();
        ops::print_json(&json!(v));
    } else {
        for (depth, i) in pairs {
            for _ in 0..*depth {
                print!("  ");
            }
            println!("{}", compact(i));
        }
    }
    Ok(0)
}

/// Input bundle for [`new_file`] — one per `<kind> new` call.
/// `word` is the commit-message noun (`chore: new issue I-001`).
pub struct NewItem<'a> {
    /// The kind of item to create.
    pub kind: ItemKind,
    /// One-line title (drives the filename slug).
    pub title: String,
    /// Body markdown — `None` → `template` fills in.
    pub body: Option<&'a str>,
    /// Optional metadata (`pri:`/`owner:`/`#label`/…).
    pub meta: &'a NewItemMeta,
    /// The kind's default body template.
    pub template: fn(&Item) -> String,
    /// Commit-message noun — `chore: new <word> <id>`.
    pub word: &'a str,
    /// `--json` output.
    pub json_out: bool,
    /// `--no-commit`.
    pub no_commit: bool,
}

/// The shared `<kind> new` body — alloc id → apply meta → body or
/// `template` → `save_item` → commit + report.
pub fn new_file(
    proj: &discover::Project,
    store: &MdStore,
    spec: &NewItem<'_>,
) -> Result<i32, String> {
    let id = store.alloc_id(spec.kind).map_err(|e| e.to_string())?;
    let mut item = Item::new(id, &spec.title).created(today());
    spec.meta.apply(&mut item)?;
    let body = spec
        .body
        .map(str::to_string)
        .unwrap_or_else(|| (spec.template)(&item));
    let file = ItemFile::new(item.clone()).body(body);
    store.save_item(&file).map_err(|e| e.to_string())?;
    let rel = item_rel_path(&item);
    let committed = commit(
        &proj.worktree,
        &format!("chore: new {} {id}", spec.word),
        spec.no_commit,
    )?;
    report(
        json!({"item": item, "path": rel}),
        committed,
        spec.json_out,
        &format!("created {id} {rel}"),
    )
}

/// The read behind [`list_files`] — live item files (plus
/// `archive/` entries when `include_archived`), filtered, as
/// data. Non-CLI front-ends (`myco serve`) shape it themselves.
pub fn query_files(
    store: &MdStore,
    kind: ItemKind,
    include_archived: bool,
    filter: &Filter,
) -> Result<Vec<Item>, String> {
    let mut found = store.list_items(kind).map_err(|e| e.to_string())?;
    if include_archived {
        found.extend(store.list_archived(kind).map_err(|e| e.to_string())?);
    }
    Ok(found
        .into_iter()
        .map(|f| f.meta)
        .filter(|i| filter.matches(i))
        .take(filter.take())
        .collect())
}

/// The shared `<kind> list` — live item files, plus `archive/`
/// entries when `include_archived`.
pub fn list_files(
    store: &MdStore,
    kind: ItemKind,
    include_archived: bool,
    filter: &Filter,
    json_out: bool,
) -> Result<i32, String> {
    let found = query_files(store, kind, include_archived, filter)?;
    print_list(&found, json_out)
}

/// `<dir>/<filename>` for a file-backed item.
pub fn item_rel_path(item: &Item) -> String {
    let dir = myco_store_md::item_dir(item.id.kind()).unwrap_or("");
    let name =
        myco_store_md::item_filename(&item.id, &item.title).unwrap_or_else(|| item.id.to_string());
    format!("{dir}/{name}")
}

/// Locate an item anywhere in the ledger — TODO.md, BACKLOG.md, or
/// an item file (live or `archive/`). Returns the item, its
/// location label, and the body for file-backed items.
pub fn find_item(
    store: &MdStore,
    id: ItemId,
) -> Result<Option<(Item, String, Option<String>)>, String> {
    let ledger = load_ledger(store)?;
    if let Some(loc) = ledger.locate(id) {
        let file = if loc.backlog { "BACKLOG.md" } else { "TODO.md" };
        return Ok(Some((ledger.item(&loc).clone(), file.to_string(), None)));
    }
    match store.item(id) {
        Ok(f) => {
            let mut loc = item_rel_path(&f.meta);
            if store.is_archived(id).map_err(|e| e.to_string())? {
                if let Some((d, n)) = loc.split_once('/') {
                    loc = format!("{d}/archive/{n}");
                }
            }
            Ok(Some((f.meta.clone(), loc, Some(f.body))))
        }
        Err(myco_store::Error::NotFound { .. }) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use myco_core::ItemKind;

    /// A plain `project/` dir that qualifies via `TODO.md` — the
    /// no-repo fallback lock path.
    fn project_dir() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let wt = dir.path().join("project");
        std::fs::create_dir_all(&wt).unwrap();
        std::fs::write(wt.join("TODO.md"), "# TODO — x\n").unwrap();
        (dir, wt)
    }

    #[test]
    fn open_serializes_mutating_verbs() {
        let (_d, wt) = project_dir();
        let (_p, store) = open(Some(wt.as_path()), None).unwrap();
        // a second writer (even in-process — the lock is per
        // open-file-description) fails fast instead of racing
        // the load→mutate→save
        let err = open(Some(wt.as_path()), None).err().unwrap();
        assert!(err.contains("another myco process is writing"), "{err}");
        // readers fail fast while a writer holds the lock…
        assert!(open_read(Some(wt.as_path()), None).is_err());
        drop(store);
        // …and everyone succeeds once it drops — a fresh writer,
        // then concurrent readers
        let (_p, store) = open(Some(wt.as_path()), None).unwrap();
        drop(store);
        let (_p2, r2) = open_read(Some(wt.as_path()), None).unwrap();
        let (_p3, r3) = open_read(Some(wt.as_path()), None).unwrap();
        drop((r2, r3));
    }

    #[test]
    fn concurrent_open_fails_fast_no_dup_ids() {
        let (_d, wt) = project_dir();
        let (_p, s1) = open(Some(wt.as_path()), None).unwrap();
        let id1 = s1.alloc_id(ItemKind::Task).unwrap();
        // a racing verb can't even open — prove the failure is
        // the lock, not the store, by retrying after the drop
        let wt2 = wt.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        let racing = std::thread::spawn(move || {
            tx.send(open(Some(wt2.as_path()), None).map(|_| ()))
                .unwrap();
        });
        let err = rx.recv().unwrap().unwrap_err();
        assert!(err.contains("another myco process"), "{err}");
        drop(s1);
        // after release, a fresh verb allocates the *next* id —
        // the lock is what kept two alloc_id RMWs from both
        // reading "next = id1" and handing out a duplicate
        let (_p, s2) = open(Some(wt.as_path()), None).unwrap();
        let id2 = s2.alloc_id(ItemKind::Task).unwrap();
        assert_ne!(id1, id2);
        racing.join().unwrap();
    }

    #[test]
    fn ledger_load_save_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let root = camino::Utf8PathBuf::from_path_buf(dir.path().join("p")).unwrap();
        std::fs::create_dir_all(&root).unwrap();
        let store = MdStore::new(root);
        let mut ledger = load_ledger(&store).unwrap();
        // repo name comes from the dir — sections start empty
        assert!(ledger.todo.sections.is_empty());
        ledger.put_todo(Item::new(ItemId::new(ItemKind::Task, 1), "x"), None);
        save_ledger(&store, &ledger).unwrap();
        let reloaded = load_ledger(&store).unwrap();
        assert!(reloaded.locate(ItemId::new(ItemKind::Task, 1)).is_some());
    }
}
