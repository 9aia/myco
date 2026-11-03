//! links — the invalidation driver for the link index (P-005).
//!
//! One [`sync`] walks a worktree's `*.md` files, diffs them
//! against the index's stored [`FileStamp`]s (mtime+size precheck,
//! then the content fingerprint), re-derives edges for the dirty
//! set only, records each file's new edge set atomically, and
//! sweeps records whose files vanished. Unchanged files are never
//! even re-read, let alone re-parsed.
//!
//! The driver is grammar-free like the rest of ops: `extract`
//! maps `(rel path, source)` to edges and is supplied by the
//! front-end that owns the file spec (the CLI's `links` module
//! for `index rebuild`; the TUI keeps its own in-memory graph).
//! A file that fails extraction is stamped with zero edges — a
//! broken file is "processed, nothing declared", not a retry
//! loop — and reported in [`SyncReport::errors`].

use std::collections::{BTreeMap, BTreeSet};

use camino::{Utf8Path, Utf8PathBuf};
use myco_core::{refs, Edge, EdgeKind, Item, ItemId};
use myco_index::{fingerprint, FileStamp, LinkIndex};
use myco_store::Store;

/// What one [`sync`] did — the counters a report renders.
#[derive(Debug, Default, Clone)]
pub struct SyncReport {
    /// `*.md` files seen live under the worktree.
    pub files: usize,
    /// Files re-derived and re-recorded (new + content-changed).
    pub dirty: usize,
    /// Files whose stamp was refreshed without a re-derive —
    /// mtime moved but the fingerprint says the content didn't.
    pub restamped: usize,
    /// Files skipped by the precheck — never re-read.
    pub clean: usize,
    /// File records dropped by [`LinkIndex::sweep`].
    pub swept: usize,
    /// Total edges recorded this sync.
    pub edges: usize,
    /// Per-file extract errors — the file was still stamped
    /// (with zero edges) so the failure isn't retried every sync.
    pub errors: Vec<String>,
}

/// One incremental link-index sync for `repo`'s worktree `wt`.
/// `repo` is the index's repo key (the canonical root path —
/// the same key snapshots use). Errors from the index propagate;
/// filesystem read errors on individual files land in
/// `report.errors` and the file is stamped clean.
pub fn sync(
    index: &impl LinkIndex,
    repo: &str,
    wt: &Utf8Path,
    extract: impl Fn(&Utf8Path, &str) -> Result<Vec<Edge>, String>,
) -> Result<SyncReport, String> {
    if !wt.is_dir() {
        return Err(format!("{wt}: not a directory — nothing to scan"));
    }
    let stored = index.stamps(repo).map_err(|e| e.to_string())?;
    let live_files = walk_md(wt)?;
    let mut live = BTreeSet::new();
    let mut report = SyncReport::default();
    for (rel, abs, meta) in &live_files {
        live.insert(rel.clone());
        let (mtime_ms, size) = stat(meta);
        // the precheck — a matching mtime+size skips the read AND
        // the hash; a store that rewrote timestamps but not bytes
        // still falls through to the fingerprint
        if stored
            .get(rel)
            .is_some_and(|s| s.mtime_ms == mtime_ms && s.size == size)
        {
            report.clean += 1;
            continue;
        }
        let bytes = match std::fs::read(abs) {
            Ok(b) => b,
            Err(e) => {
                report.errors.push(format!("{rel}: {e}"));
                continue;
            }
        };
        let stamp = FileStamp {
            mtime_ms,
            size,
            fingerprint: fingerprint(&bytes),
        };
        if stored
            .get(rel)
            .is_some_and(|s| s.fingerprint == stamp.fingerprint)
        {
            // touched, not changed — restamp so the next precheck
            // stays cheap; edges are already right
            index
                .restamp(repo, rel, &stamp)
                .map_err(|e| e.to_string())?;
            report.restamped += 1;
            continue;
        }
        let src = String::from_utf8_lossy(&bytes);
        let edges = match extract(rel, &src) {
            Ok(e) => e,
            Err(e) => {
                report.errors.push(format!("{rel}: {e}"));
                Vec::new()
            }
        };
        report.edges += edges.len();
        index
            .record(repo, rel, &stamp, &edges)
            .map_err(|e| e.to_string())?;
        report.dirty += 1;
    }
    report.swept = index.sweep(repo, &live).map_err(|e| e.to_string())?;
    report.files = live.len();
    Ok(report)
}

/// Every `*.md` file under `wt` as `(rel, abs, metadata)`, sorted
/// by rel path for deterministic processing order. Dot-dirs,
/// symlinked entries, and non-files are skipped — the ledger is
/// plain files.
fn walk_md(wt: &Utf8Path) -> Result<Vec<(Utf8PathBuf, Utf8PathBuf, std::fs::Metadata)>, String> {
    let mut out = Vec::new();
    let mut stack = vec![wt.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let read = match std::fs::read_dir(&dir) {
            Ok(r) => r,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(format!("{dir}: {e}")),
        };
        for entry in read {
            let entry = entry.map_err(|e| format!("{dir}: {e}"))?;
            let ft = entry.file_type().map_err(|e| format!("{dir}: {e}"))?;
            if ft.is_symlink() {
                continue;
            }
            let Ok(path) = Utf8PathBuf::from_path_buf(entry.path()) else {
                continue;
            };
            if ft.is_dir() {
                if !path.file_name().is_some_and(|n| n.starts_with('.')) {
                    stack.push(path);
                }
            } else if ft.is_file() && path.extension() == Some("md") {
                let meta = entry.metadata().map_err(|e| format!("{path}: {e}"))?;
                let rel = path
                    .strip_prefix(wt)
                    .map_err(|_| format!("{path}: outside worktree"))?
                    .to_path_buf();
                out.push((rel, path, meta));
            }
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

/// `(mtime_ms, size)` — the stamp's stat-level precheck half.
fn stat(meta: &std::fs::Metadata) -> (u64, u64) {
    let mtime_ms = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0);
    (mtime_ms, meta.len())
}

// ---------------------------------------------------------------- live graph

/// The link graph derived live from a store — the same edge set
/// [`sync`] would record plus a title per known item. This is the
/// read side `item show` and `serve item.show` use instead of the
/// persisted index, deliberately:
///
/// - The question is bounded — "what touches `id`?" — and the
///   store is already open; a full gather is one ledger scan,
///   which `item show` mostly pays for anyway.
/// - A live read has no registration or staleness question —
///   `item show` works on repos `index add` never saw, and never
///   shows edges older than the file on disk.
/// - A read verb stays read-only: `sync` writes stamps/edges into
///   the shared redb (taking its lock), which would make `show`
///   mutate global state and contend with real writers.
///
/// The persisted index exists for the queries that outnumber
/// ledger scans — repeated lookups, cross-repo views — not for a
/// single item's neighborhood.
pub struct LiveGraph {
    /// Every edge the ledger declares, `(src, kind, dst)`-sorted —
    /// frontmatter links, prose mentions, handoff attachments;
    /// live *and* archived items (an archived ADR still supersedes).
    pub edges: Vec<Edge>,
    /// Item id → title, for annotating neighbors in output.
    pub titles: BTreeMap<ItemId, String>,
}

/// One neighbor of an item, as `related` reports it — the
/// query-side view of [`myco_index::Related`] with the title
/// resolved while the graph is in hand. `kind` is already
/// mirrored for incoming edges.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelatedItem {
    /// The relationship as seen from the queried item —
    /// `blocked_by`/`referenced_by` for incoming edges.
    pub kind: EdgeKind,
    /// The item at the other end of the edge.
    pub other: ItemId,
    /// `other`'s title when the ledger knows it — a dangling edge
    /// end (a mention of an id that doesn't exist) carries `None`
    /// and renders as the bare id, not a filtered-away edge.
    pub title: Option<String>,
}

impl LiveGraph {
    /// Everything attached to `id`, both directions — outgoing
    /// kinds verbatim, incoming mirrored (`blocks` → `blocked_by`),
    /// sorted by `(kind, other)`. Unlike [`LinkIndex::related`]
    /// there's no `alive` filter: the TUI's in-memory graph has
    /// the same contract, and a dangling id in the output is
    /// information (`check` repairs it), not corruption.
    pub fn related(&self, id: ItemId) -> Vec<RelatedItem> {
        let mut out: Vec<RelatedItem> = Vec::new();
        for e in &self.edges {
            let (kind, other) = if e.src == id {
                (e.kind, e.dst)
            } else if e.dst == id {
                (e.kind.mirror(), e.src)
            } else {
                continue;
            };
            out.push(RelatedItem {
                kind,
                other,
                title: self.titles.get(&other).cloned(),
            });
        }
        out.sort_by_key(|r| (r.kind, r.other));
        out.dedup_by(|a, b| a.kind == b.kind && a.other == b.other);
        out
    }
}

/// Derive the [`LiveGraph`] over a store — TODO/BACKLOG items
/// (whole subtrees, titles scanned for mentions), every item file
/// (live + `archive/`, metas + bodies), and every `handoffs/*.md`
/// (the filename self-edge plus mentions inside entry notes).
/// Per-source edge sets are what `extract_file` emits per path;
/// gathering through [`Store`] ports it off the `*.md` walk so
/// non-fs stores play too.
///
/// Per-source lenient: this feeds display surfaces (`item show`),
/// so one corrupt file costs its own edges, not the whole graph —
/// a source that fails to read contributes nothing and its error
/// lands in the returned `warnings` (drift is `myco check`'s
/// report, not a display verb's abort).
pub fn live_graph(store: &impl Store) -> (LiveGraph, Vec<String>) {
    let mut edges = BTreeSet::new();
    let mut titles = BTreeMap::new();
    let mut warnings = Vec::new();

    match crate::items::load_ledger(store) {
        Ok(ledger) => {
            for i in ledger
                .todo
                .items()
                .chain(ledger.backlog.items())
                .flat_map(Item::subtree)
            {
                titles.insert(i.id, i.title.clone());
                edges.extend(refs::extract(&i.title, Some(i), i.id, store.kinds()));
            }
        }
        Err(e) => warnings.push(format!("ledger: {e}")),
    }

    for spec in store.kinds().file_backed() {
        // live and archived gather independently — a corrupt file
        // in one scope shouldn't blind the other
        for (scope, files) in [
            ("", store.list_items(spec.id_kind())),
            ("archived ", store.list_archived(spec.id_kind())),
        ] {
            match files {
                Ok(files) => {
                    for f in files {
                        titles.insert(f.meta.id, f.meta.title.clone());
                        edges.extend(refs::extract(
                            &f.body,
                            Some(&f.meta),
                            f.meta.id,
                            store.kinds(),
                        ));
                    }
                }
                Err(e) => warnings.push(format!("{scope}{} files: {e}", spec.name)),
            }
        }
    }

    match store.list_handoff_files() {
        Ok(handoffs) => {
            for h in handoffs {
                // the filename fact survives even when the notes
                // don't parse — the file exists either way
                edges.insert(Edge::handoff(h.id));
                match store.read_handoffs(h.id) {
                    Ok(entries) => {
                        let notes = entries
                            .iter()
                            .map(|e| e.note.as_str())
                            .collect::<Vec<_>>()
                            .join("\n");
                        edges.extend(refs::extract(&notes, None, h.id, store.kinds()));
                    }
                    Err(e) => warnings.push(format!("handoffs/{}.md: {e}", h.id)),
                }
            }
        }
        Err(e) => warnings.push(format!("handoffs: {e}")),
    }

    (
        LiveGraph {
            edges: edges.into_iter().collect(),
            titles,
        },
        warnings,
    )
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use myco_core::{EdgeKind, ItemId, ItemKind};
    use myco_index_redb::RedbIndex;
    use std::collections::BTreeMap;

    /// A repo-shaped worktree + a throwaway index.
    fn setup() -> (tempfile::TempDir, RedbIndex, Utf8PathBuf) {
        let d = tempfile::tempdir().unwrap();
        let wt = Utf8PathBuf::from_path_buf(d.path().join("project")).unwrap();
        std::fs::create_dir_all(&wt).unwrap();
        let idx = Utf8PathBuf::from_path_buf(d.path().join("idx")).unwrap();
        (d, RedbIndex::open_at(idx).unwrap(), wt)
    }

    fn tid(n: u32) -> ItemId {
        ItemId::new(ItemKind::Task, n)
    }

    /// A trivial extractor: `<stem>` parses to an id → a self
    /// `references` edge to T-1; "boom" files error.
    fn stub_extract(rel: &Utf8Path, _src: &str) -> Result<Vec<Edge>, String> {
        if rel.as_str().contains("boom") {
            return Err("kaboom".into());
        }
        Ok(vec![Edge::new(tid(9), tid(1), EdgeKind::References)])
    }

    #[test]
    fn sync_records_only_dirty_files() {
        let (_d, idx, wt) = setup();
        std::fs::write(wt.join("a.md"), "alpha").unwrap();
        std::fs::write(wt.join("b.md"), "beta").unwrap();
        let r = sync(&idx, "/r", &wt, stub_extract).unwrap();
        assert_eq!((r.files, r.dirty, r.clean, r.swept), (2, 2, 0, 0));
        assert_eq!(r.edges, 2);
        assert_eq!(idx.stamps("/r").unwrap().len(), 2);

        // second sync — nothing changed, nothing re-derived
        let r = sync(&idx, "/r", &wt, stub_extract).unwrap();
        assert_eq!((r.dirty, r.clean, r.edges), (0, 2, 0));

        // one file changes → only it is re-derived
        std::fs::write(wt.join("b.md"), "beta v2").unwrap();
        let r = sync(&idx, "/r", &wt, stub_extract).unwrap();
        assert_eq!((r.dirty, r.clean), (1, 1));

        // a vanishing file sweeps its record + edges
        std::fs::remove_file(wt.join("a.md")).unwrap();
        let r = sync(&idx, "/r", &wt, stub_extract).unwrap();
        assert_eq!(r.swept, 1);
        assert_eq!(idx.stamps("/r").unwrap().len(), 1);
    }

    #[test]
    fn sync_restamps_touched_files() {
        let (_d, idx, wt) = setup();
        let f = wt.join("a.md");
        std::fs::write(&f, "alpha").unwrap();
        sync(&idx, "/r", &wt, stub_extract).unwrap();
        // same bytes, new mtime — must not re-derive (edges count
        // stays put) but the stamp must move so the next sync
        // takes the precheck again
        std::fs::write(&f, "alpha").unwrap();
        let r = sync(&idx, "/r", &wt, stub_extract).unwrap();
        // same content — either the mtime moved (restamp) or it
        // didn't (clean precheck hit); never a re-derive
        assert!(
            r.restamped == 1 || r.clean == 1,
            "touch must restamp or pass clean, got {r:?}"
        );
        assert_eq!(r.dirty, 0);
    }

    #[test]
    fn extract_errors_land_in_report_and_stamp_anyway() {
        let (_d, idx, wt) = setup();
        std::fs::write(wt.join("boom.md"), "broken").unwrap();
        let r = sync(&idx, "/r", &wt, stub_extract).unwrap();
        assert_eq!(r.errors.len(), 1);
        assert_eq!(r.dirty, 1); // still recorded — zero edges
                                // the file doesn't re-fail every sync — it's stamped
        let r = sync(&idx, "/r", &wt, stub_extract).unwrap();
        assert!(r.errors.is_empty());
        assert_eq!(r.dirty, 0);
    }

    #[test]
    fn nested_and_dot_files() {
        let (_d, idx, wt) = setup();
        std::fs::create_dir_all(wt.join("issues/archive")).unwrap();
        std::fs::create_dir_all(wt.join(".git")).unwrap();
        std::fs::write(wt.join("issues/archive/i.md"), "x").unwrap();
        std::fs::write(wt.join(".git/HEAD.md"), "x").unwrap();
        std::fs::write(wt.join("not-md.txt"), "x").unwrap();
        let r = sync(&idx, "/r", &wt, stub_extract).unwrap();
        // only the nested .md — dot dirs and non-md skipped
        assert_eq!(r.files, 1);
        let mut live = BTreeMap::new();
        live.insert(
            Utf8PathBuf::from("issues/archive/i.md"),
            idx.stamps("/r").unwrap()[&Utf8PathBuf::from("issues/archive/i.md")],
        );
        assert!(idx.dirty("/r", &live).unwrap().is_empty());
    }

    #[test]
    fn missing_worktree_is_an_error_not_a_purge() {
        let (_d, idx, wt) = setup();
        // a vanished worktree errors — sync never purges on
        // absence alone (a transient mount failure shouldn't nuke
        // the index)
        let r = sync(&idx, "/r", &wt.join("nope"), stub_extract);
        assert!(r.is_err());
    }

    // ------------------------------------------------ live graph

    use myco_core::{HandoffEntry, Item, ItemFile};
    use myco_store_md::MdStore;

    /// A store over a tempdir worktree — the tests name the impl,
    /// like the composition root does.
    fn store() -> (tempfile::TempDir, MdStore) {
        let d = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(d.path().to_path_buf()).unwrap();
        (d, MdStore::new(root))
    }

    fn item(id: ItemId, title: &str) -> Item {
        Item::new(id, title)
    }

    fn entry(note: &str) -> HandoffEntry {
        HandoffEntry {
            date: myco_core::date::parse("2026-10-29").unwrap(),
            actor: myco_core::Actor::new("devin").unwrap(),
            session: None,
            note: note.into(),
        }
    }

    /// `related(id)` → `(kind, other, title)` tuples — assertion
    /// shorthand.
    fn rels(g: &LiveGraph, id: ItemId) -> Vec<(EdgeKind, ItemId, Option<String>)> {
        g.related(id)
            .into_iter()
            .map(|r| (r.kind, r.other, r.title))
            .collect()
    }

    #[test]
    fn live_graph_gathers_every_source() {
        let (_d, s) = store();
        let mut ledger = crate::items::load_ledger(&s).unwrap();
        ledger.put_todo(item(tid(1), "alpha").blocks(tid(2)), None);
        ledger.put_todo(item(tid(2), "beta — see T-009"), None);
        crate::items::save_ledger(&s, &ledger).unwrap();
        let i3 = ItemId::new(ItemKind::Issue, 3);
        s.save_item(&ItemFile::new(item(i3, "the issue")).body("blocked by T-001"))
            .unwrap();
        s.append_handoff(tid(1), &entry("wip — unblocks I-003"))
            .unwrap();

        let (g, warnings) = live_graph(&s);
        assert!(warnings.is_empty(), "{warnings:?}");
        // titles resolve for every known item
        assert_eq!(g.titles[&tid(1)], "alpha");
        assert_eq!(g.titles[&i3], "the issue");

        // T-001's neighborhood: outgoing blocks + handoff self-edge +
        // the note's mention, incoming mirrored references (file body)
        assert_eq!(
            rels(&g, tid(1)),
            [
                (EdgeKind::Handoff, tid(1), Some("alpha".to_string())),
                (
                    EdgeKind::Blocks,
                    tid(2),
                    Some("beta — see T-009".to_string())
                ),
                (EdgeKind::References, i3, Some("the issue".to_string())),
                (EdgeKind::ReferencedBy, i3, Some("the issue".to_string())),
            ]
        );
        // the mirror reads from T-002's side as blocked_by; its own
        // mention of a dangling id resolves to no title but still
        // surfaces (check's job to repair, not ours to hide)
        assert_eq!(
            rels(&g, tid(2)),
            [
                (EdgeKind::BlockedBy, tid(1), Some("alpha".to_string())),
                (EdgeKind::References, tid(9), None),
            ]
        );
    }

    #[test]
    fn live_graph_sees_archived_and_ignores_the_unknown() {
        let (_d, s) = store();
        let adr1 = ItemId::new(ItemKind::Adr, 1);
        let adr2 = ItemId::new(ItemKind::Adr, 2);
        s.save_item(&ItemFile::new(item(adr1, "old decision")))
            .unwrap();
        s.save_item(
            &ItemFile::new(item(adr2, "new decision").supersedes(adr1)).body("replaces ADR-0001"),
        )
        .unwrap();
        s.archive_item(adr1).unwrap();

        let (g, warnings) = live_graph(&s);
        assert!(warnings.is_empty(), "{warnings:?}");
        // an archived item still declares (and receives) edges —
        // supersession is history, and history is what archive holds
        assert_eq!(
            rels(&g, adr1),
            [
                (
                    EdgeKind::SupersededBy,
                    adr2,
                    Some("new decision".to_string())
                ),
                (
                    EdgeKind::ReferencedBy,
                    adr2,
                    Some("new decision".to_string())
                ),
            ]
        );
        // nothing attached → empty, not an error
        assert!(g.related(tid(9)).is_empty());
    }

    #[test]
    fn live_graph_leniency_costs_only_the_corrupt_source() {
        let (_d, s) = store();
        let i3 = ItemId::new(ItemKind::Issue, 3);
        s.save_item(&ItemFile::new(item(i3, "good"))).unwrap();
        // an unparseable issue file kills `list_items(Issue)` but
        // nothing else — the graph keeps its other sources
        let bad = s.root().join("issues/issue-0009-bad.md");
        std::fs::write(&bad, "+++\nid = \"I-009\"\ntitle = [oops\n+++\n").unwrap();
        let mut ledger = crate::items::load_ledger(&s).unwrap();
        ledger.put_todo(item(tid(1), "alpha").blocks(tid(2)), None);
        crate::items::save_ledger(&s, &ledger).unwrap();

        let (g, warnings) = live_graph(&s);
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(warnings[0].contains("issue files"), "{warnings:?}");
        // ledger edges still gathered; the bad file's neighbors
        // just lose their issue-kind edges + titles
        assert_eq!(rels(&g, tid(1)), [(EdgeKind::Blocks, tid(2), None)]);
    }
}
