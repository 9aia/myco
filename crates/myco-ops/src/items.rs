//! Item ops — the I/O shell around `myco-core`'s domain: store
//! open/save, the commit-per-mutation tail, item-file reads, and
//! the shared output verbs. The pure pieces (ledger doc surgery,
//! state walks, `compact`, `tree_view`, `require_kind`,
//! `NewItemMeta`) live in `myco-core` and are re-exported here so
//! call sites read `items::X` either way.

use std::path::Path;

use myco_core::{Item, ItemFile, ItemId, ItemKind, Priority, Status};
use myco_store::Store;
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
pub fn load_ledger(store: &impl Store) -> Result<Ledger, String> {
    Ok(Ledger::new(
        store.todo().map_err(|e| e.to_string())?,
        store.backlog().map_err(|e| e.to_string())?,
    ))
}

/// Write both ledger docs back through the store.
pub fn save_ledger(store: &impl Store, ledger: &Ledger) -> Result<(), String> {
    store.save_todo(&ledger.todo).map_err(|e| e.to_string())?;
    store
        .save_backlog(&ledger.backlog)
        .map_err(|e| e.to_string())
}

/// `DONE.md` — the done-task archive (`todo done` lands here).
pub fn load_done(store: &impl Store) -> Result<myco_core::DoneDoc, String> {
    store.done().map_err(|e| e.to_string())
}

/// Write `DONE.md` back through the store.
pub fn save_done(store: &impl Store, doc: &myco_core::DoneDoc) -> Result<(), String> {
    store.save_done(doc).map_err(|e| e.to_string())
}

/// Append `item` (subtree included — every node must already be
/// terminal, `Done` or `Cancelled`; emit rejects anything else)
/// to the done doc's flat unsectioned list. Duplicate ids — the
/// item's whole
/// subtree — are refused: an id lives in exactly one place.
pub fn done_push(doc: &mut myco_core::DoneDoc, item: Item) -> Result<(), String> {
    let live: std::collections::BTreeSet<_> = doc
        .items()
        .flat_map(|i| i.subtree())
        .map(|i| i.id)
        .collect();
    if item.subtree().any(|i| live.contains(&i.id)) {
        return Err(format!("{}: already in DONE.md", item.id));
    }
    // the flat list lives in the unsectioned run — it must stay
    // at index 0 (hand-edited `##` sections may precede nothing)
    match doc.sections.first_mut() {
        Some(s) if s.name.is_none() => s.items.push(item),
        _ => doc.sections.insert(
            0,
            myco_core::Section {
                name: None,
                items: vec![item],
            },
        ),
    }
    Ok(())
}

/// Archive a done task subtree into `DONE.md` — load, push, save.
/// Callers that must validate *before* any write stage the doc
/// themselves (`load_done` → [`done_push`] → `save_ledger` →
/// `save_done`) — this convenience form loads fresh, so a dup id
/// surfaces only after earlier writes landed.
pub fn archive_done(store: &impl Store, item: Item) -> Result<(), String> {
    let mut doc = load_done(store)?;
    done_push(&mut doc, item)?;
    save_done(store, &doc)
}

/// Load a file-backed item for mutation. Archived items are
/// refused — `save_item` writes to the live dir, so editing one
/// would silently un-archive it.
pub fn item_for_edit(store: &impl Store, id: ItemId) -> Result<ItemFile, String> {
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
/// [`open_with`] (mutating verbs), shared from [`open_read_with`].
/// The lock releases when this drops. Derefs to the inner store,
/// so `&locked` coerces to `&S` at every call site. Generic over
/// the store — the front-end (composition root) names the impl.
pub struct LockedStore<S> {
    store: S,
    _lock: Option<ops::WorktreeLock>,
}

impl<S> LockedStore<S> {
    /// A store holding no lock — for tests and callers that
    /// guarantee a single writer by construction.
    pub fn unlocked(store: S) -> Self {
        LockedStore { store, _lock: None }
    }
}

impl<S> std::ops::Deref for LockedStore<S> {
    type Target = S;

    fn deref(&self) -> &S {
        &self.store
    }
}

/// `LockedStore` is a full [`Store`] — pure delegation, so
/// `&locked` satisfies `&impl Store` bounds (generic params don't
/// deref-coerce).
impl<S: Store> Store for LockedStore<S> {
    fn kinds(&self) -> &myco_core::KindTable {
        self.store.kinds()
    }

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
    fn done(&self) -> Result<myco_core::DoneDoc, myco_store::Error> {
        self.store.done()
    }
    fn save_done(&self, doc: &myco_core::DoneDoc) -> Result<(), myco_store::Error> {
        self.store.save_done(doc)
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
    fn item_path(&self, item: &Item) -> Result<Option<String>, myco_store::Error> {
        self.store.item_path(item)
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

/// Resolve the project + open the store `mk` builds under the
/// exclusive mutation lock — every verb that writes the ledger
/// takes this path. `mk` gets the discovered worktree and names
/// the concrete impl (`myco`'s `ops::store_at` → `MdStore`), so
/// this crate stays generic over [`Store`]; `cfg` is the same
/// story for config (discovery reads `project_dir`). The lock
/// spans discovery → mutation → the per-verb autocommit;
/// contention fails fast (`another myco process is writing —
/// retry`) instead of corrupting the ledger.
pub fn open_with<S: Store>(
    dir: Option<&Path>,
    root: Option<&Path>,
    cfg: crate::config::ConfigLoader,
    mk: impl FnOnce(&Path, &myco_core::KindTable) -> Result<S, String>,
) -> Result<(discover::Project, LockedStore<S>), String> {
    open_with_index(dir, root, cfg, None, mk)
}

/// [`open_with`] with the repo index at discovery's disposal —
/// the org-ledger fallback: a *registered* repo with no ledger
/// of its own resolves to the `ledger = "org"` entry's worktree.
/// `None` keeps discovery strict (unregistered repos fail).
pub fn open_with_index<S: Store>(
    dir: Option<&Path>,
    root: Option<&Path>,
    cfg: crate::config::ConfigLoader,
    index: Option<&dyn myco_index::Index>,
    mk: impl FnOnce(&Path, &myco_core::KindTable) -> Result<S, String>,
) -> Result<(discover::Project, LockedStore<S>), String> {
    let proj = discover::discover_with(dir, root, cfg, index)?;
    // a detached worktree would swallow the write — refuse before
    // the store opens (`check --fix` repairs the worktree itself)
    ops::ensure_committable(&proj.worktree)?;
    // a ledger behind its remote must sync before mutating —
    // every verb commit would otherwise deepen the divergence.
    // `sync.auto_fetch` (default on) refreshes the remote-tracking
    // ref first — best-effort, a failed fetch never blocks.
    ops::ensure_synced(
        &proj.worktree,
        crate::config::sync_auto_fetch_for(&proj.root, cfg),
    )?;
    let kinds = crate::config::kinds_for(&proj.root, cfg);
    let lock = ops::lock_worktree(&proj.worktree)?;
    let store = mk(&proj.worktree, &kinds)?;
    Ok((
        proj,
        LockedStore {
            store,
            _lock: Some(lock),
        },
    ))
}

/// [`open_with`] under the shared read lock — read-only verbs
/// (`status`, `view`, `list`, `resume`, …). Readers don't block
/// each other; a reader meeting a live writer fails fast.
pub fn open_read_with<S: Store>(
    dir: Option<&Path>,
    root: Option<&Path>,
    cfg: crate::config::ConfigLoader,
    mk: impl FnOnce(&Path, &myco_core::KindTable) -> Result<S, String>,
) -> Result<(discover::Project, LockedStore<S>), String> {
    open_read_with_index(dir, root, cfg, None, mk)
}

/// [`open_read_with`] with the repo index — see
/// [`open_with_index`].
pub fn open_read_with_index<S: Store>(
    dir: Option<&Path>,
    root: Option<&Path>,
    cfg: crate::config::ConfigLoader,
    index: Option<&dyn myco_index::Index>,
    mk: impl FnOnce(&Path, &myco_core::KindTable) -> Result<S, String>,
) -> Result<(discover::Project, LockedStore<S>), String> {
    let proj = discover::discover_with(dir, root, cfg, index)?;
    let kinds = crate::config::kinds_for(&proj.root, cfg);
    let lock = ops::lock_shared(&proj.worktree)?;
    let store = mk(&proj.worktree, &kinds)?;
    Ok((
        proj,
        LockedStore {
            store,
            _lock: Some(lock),
        },
    ))
}

/// The standard mutation tail — one atomic commit on the worktree.
/// `actor` is the verb's actor (`--actor`/`--owner` where the verb
/// takes one) — stamped as the `Actor:` trailer; `None` → `cli`.
pub fn commit(
    wt: &Path,
    msg: &str,
    actor: Option<&myco_core::Actor>,
    no_commit: bool,
) -> Result<bool, String> {
    ops::autocommit(wt, msg, actor, ops::commits_enabled(no_commit))
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

/// `--status/--owner/--label/--pri/--epic/--milestone/--due/--est/
/// --cycle/--query` list filter + `--limit` — the parsed form of
/// the CLI's `ListFlags`, usable by any front-end.
#[derive(Debug, Clone, Default)]
pub struct Filter {
    /// Keep only items with this status.
    pub status: Option<Status>,
    /// Keep only items owned by this actor string.
    pub owner: Option<String>,
    /// Keep only items carrying all of these labels.
    pub labels: Vec<String>,
    /// Keep only items at this priority.
    pub pri: Option<Priority>,
    /// Keep only items in this epic.
    pub epic: Option<String>,
    /// Keep only items in this milestone.
    pub milestone: Option<String>,
    /// Keep only items due on this exact date.
    pub due: Option<time::Date>,
    /// Keep only items with this point estimate.
    pub estimate: Option<u32>,
    /// Keep only items in this cycle.
    pub cycle: Option<String>,
    /// Keep only items whose id, title, or a label contains this
    /// text (case-insensitive).
    pub query: Option<String>,
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
            && self.pri.is_none_or(|p| item.priority == p)
            && self
                .epic
                .as_deref()
                .is_none_or(|e| item.epic.as_deref() == Some(e))
            && self
                .milestone
                .as_deref()
                .is_none_or(|m| item.milestone.as_deref() == Some(m))
            && self.due.is_none_or(|d| item.due == Some(d))
            && self.estimate.is_none_or(|e| item.estimate == Some(e))
            && self
                .cycle
                .as_deref()
                .is_none_or(|c| item.cycle.as_deref() == Some(c))
            && self.query.as_deref().is_none_or(|q| {
                let q = q.to_lowercase();
                item.id.to_string().to_lowercase().contains(&q)
                    || item.title.to_lowercase().contains(&q)
                    || item.labels.iter().any(|l| l.to_lowercase().contains(&q))
            })
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
    store: &impl Store,
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
    let rel = item_rel_path(store, &item)?;
    let committed = commit(
        &proj.worktree,
        &format!("chore: new {} {id}", spec.word),
        spec.meta.owner.as_ref(),
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
/// `(item, archived)` pairs. Non-CLI front-ends (`myco serve`)
/// shape it themselves — [`marked_json`] is the list shape.
pub fn query_files(
    store: &impl Store,
    kind: ItemKind,
    include_archived: bool,
    filter: &Filter,
) -> Result<Vec<(Item, bool)>, String> {
    let live = store.list_items(kind).map_err(|e| e.to_string())?;
    let mut found: Vec<(Item, bool)> = live.into_iter().map(|f| (f.meta, false)).collect();
    if include_archived {
        found.extend(
            store
                .list_archived(kind)
                .map_err(|e| e.to_string())?
                .into_iter()
                .map(|f| (f.meta, true)),
        );
    }
    Ok(found
        .into_iter()
        .filter(|(i, _)| filter.matches(i))
        .take(filter.take())
        .collect())
}

/// `(item, archived)` pairs → a bare array of item objects, each
/// tagged `"archived": bool` — the `--json` list shape. Live and
/// archived items share one array; the flag is the marker.
pub fn marked_json(items: &[(Item, bool)]) -> serde_json::Value {
    json!(items
        .iter()
        .map(|(i, archived)| {
            let mut v = serde_json::to_value(i).unwrap_or(json!(null));
            if let Some(obj) = v.as_object_mut() {
                obj.insert("archived".into(), json!(archived));
            }
            v
        })
        .collect::<Vec<_>>())
}

/// The shared `<kind> list` — live item files, plus `archive/`
/// entries when `include_archived`.
pub fn list_files(
    store: &impl Store,
    kind: ItemKind,
    include_archived: bool,
    filter: &Filter,
    json_out: bool,
) -> Result<i32, String> {
    let found = query_files(store, kind, include_archived, filter)?;
    if json_out {
        ops::print_json(&marked_json(&found));
        return Ok(0);
    }
    let items: Vec<Item> = found.into_iter().map(|(i, _)| i).collect();
    print_list(&items, false)
}

/// `<dir>/<filename>` for a file-backed item — the store impl's
/// own naming grammar ([`Store::item_path`], archive scope
/// included), falling back to the bare id for kinds with no file
/// form.
pub fn item_rel_path(store: &impl Store, item: &Item) -> Result<String, String> {
    Ok(store
        .item_path(item)
        .map_err(|e| e.to_string())?
        .unwrap_or_else(|| item.id.to_string()))
}

/// Locate an item anywhere in the ledger — TODO.md, BACKLOG.md, or
/// an item file (live or `archive/`). Returns the item, its
/// location label, and the body for file-backed items.
pub fn find_item(
    store: &impl Store,
    id: ItemId,
) -> Result<Option<(Item, String, Option<String>)>, String> {
    let ledger = load_ledger(store)?;
    if let Some(loc) = ledger.locate(id) {
        let file = if loc.backlog { "BACKLOG.md" } else { "TODO.md" };
        if let Some(item) = ledger.item(&loc) {
            return Ok(Some((item.clone(), file.to_string(), None)));
        }
    }
    // DONE.md — finished tasks stay resolvable (`item show`,
    // dependents, links) after they leave the live ledger
    let done = load_done(store)?;
    if let Some(item) = done.items().flat_map(|i| i.subtree()).find(|i| i.id == id) {
        return Ok(Some((item.clone(), "DONE.md".to_string(), None)));
    }
    match store.item(id) {
        Ok(f) => {
            let loc = item_rel_path(store, &f.meta)?;
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
    use myco_store_md::MdStore;

    /// `MdStore` rooted at a worktree path — the tests name the
    /// impl, like the CLI's `ops::store_at` does.
    fn md_store(wt: &Path, _kinds: &myco_core::KindTable) -> Result<MdStore, String> {
        let root = camino::Utf8PathBuf::from_path_buf(wt.to_path_buf())
            .map_err(|p| format!("{}: non-utf8 path", p.display()))?;
        Ok(MdStore::new(root))
    }

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
        let (_p, store) = open_with(
            Some(wt.as_path()),
            None,
            crate::config::test_loader(),
            md_store,
        )
        .unwrap();
        // a second writer (even in-process — the lock is per
        // open-file-description) fails fast instead of racing
        // the load→mutate→save
        let err = open_with(
            Some(wt.as_path()),
            None,
            crate::config::test_loader(),
            md_store,
        )
        .err()
        .unwrap();
        assert!(err.contains("another myco process is writing"), "{err}");
        // readers fail fast while a writer holds the lock…
        assert!(open_read_with(
            Some(wt.as_path()),
            None,
            crate::config::test_loader(),
            md_store
        )
        .is_err());
        drop(store);
        // …and everyone succeeds once it drops — a fresh writer,
        // then concurrent readers
        let (_p, store) = open_with(
            Some(wt.as_path()),
            None,
            crate::config::test_loader(),
            md_store,
        )
        .unwrap();
        drop(store);
        let (_p2, r2) = open_read_with(
            Some(wt.as_path()),
            None,
            crate::config::test_loader(),
            md_store,
        )
        .unwrap();
        let (_p3, r3) = open_read_with(
            Some(wt.as_path()),
            None,
            crate::config::test_loader(),
            md_store,
        )
        .unwrap();
        drop((r2, r3));
    }

    #[test]
    fn concurrent_open_fails_fast_no_dup_ids() {
        let (_d, wt) = project_dir();
        let (_p, s1) = open_with(
            Some(wt.as_path()),
            None,
            crate::config::test_loader(),
            md_store,
        )
        .unwrap();
        let id1 = s1.alloc_id(ItemKind::Task).unwrap();
        // a racing verb can't even open — prove the failure is
        // the lock, not the store, by retrying after the drop
        let wt2 = wt.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        let racing = std::thread::spawn(move || {
            tx.send(
                open_with(
                    Some(wt2.as_path()),
                    None,
                    crate::config::test_loader(),
                    md_store,
                )
                .map(|_| ()),
            )
            .unwrap();
        });
        let err = rx.recv().unwrap().unwrap_err();
        assert!(err.contains("another myco process"), "{err}");
        drop(s1);
        // after release, a fresh verb allocates the *next* id —
        // the lock is what kept two alloc_id RMWs from both
        // reading "next = id1" and handing out a duplicate
        let (_p, s2) = open_with(
            Some(wt.as_path()),
            None,
            crate::config::test_loader(),
            md_store,
        )
        .unwrap();
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
