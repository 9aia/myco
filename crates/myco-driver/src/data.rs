//! data — one read of a repo's `project/` worktree per wire op.
//!
//! The driver is a *reader*: every `view`/`query` resolves the
//! target project, takes the shared worktree lock for the span of
//! the load (same discipline as `myco status`), and drops it. No
//! mutations, no index — `myco serve` stays the write/embedding
//! surface; this crate only needs what the TUI rendered.
//!
//! Project resolution order (first hit wins):
//!
//! 1. Request params — `dir` (a `project/` worktree path) or
//!    `root` (a repo root; `<root>/project` is tried), e.g.
//!    `pavo view myco:board --param dir=/path/to/repo/project`.
//! 2. `$MYCO_DIR` — treated as `dir`.
//! 3. cwd discovery — `myco_ops::discover` walks up from the
//!    driver's cwd for a qualifying `project/` worktree (the same
//!    rules `myco` verbs use, including `myco.toml`'s
//!    `project_dir`).
//!
//! When nothing resolves, [`Snapshot::error`] carries the reason
//! and every query degrades to an `{"error": …}` payload — the
//! wire stays `ok:true` so hosts render the failure inline.

use std::path::{Path, PathBuf};

use myco_core::{ItemFile, ItemId, KindTable, Ledger};
use myco_ops::{
    discover, items,
    links::{self, LiveGraph},
};
use myco_store::Store;
use myco_store_md::MdStore;

/// The config impl ops calls share — the scarab-backed TOML
/// loader `myco-ops` owns, same as the CLI's `ops::CONFIG`.
pub const CONFIG: myco_ops::config::ConfigLoader = myco_ops::config::TOML;

/// `MdStore` rooted at a worktree path, resolving item layout
/// through the repo's kind table — identical to the CLI's
/// `ops::store_at` (kept local: that helper lives in the `myco`
/// bin crate, which a library consumer can't link).
fn store_at(worktree: &Path, kinds: &KindTable) -> Result<MdStore, String> {
    let root = camino::Utf8PathBuf::from_path_buf(worktree.to_path_buf())
        .map_err(|p| format!("{}: non-utf8 path", p.display()))?;
    Ok(MdStore::with_kinds(root, kinds.clone()))
}

/// One handoff file's row for the `handoffs` view.
#[derive(Debug, Clone)]
pub struct HandoffRow {
    /// The item the file records handoffs for.
    pub id: ItemId,
    /// Entry count.
    pub entries: usize,
    /// Lives under `handoffs/archive/`.
    pub archived: bool,
    /// Latest entry's `YYYY-MM-DD`.
    pub updated: Option<String>,
    /// Latest entry's actor.
    pub actor: Option<String>,
    /// Latest entry's note, truncated for table display.
    pub note: Option<String>,
}

/// Everything the views need, loaded once per op.
pub struct Snapshot {
    /// The resolved project — `None` when discovery/open failed.
    pub project: Option<discover::Project>,
    /// TODO.md + BACKLOG.md.
    pub ledger: Option<Ledger>,
    /// Live file-backed items (issues/proposals/plans/adrs +
    /// user kinds), metas + bodies.
    pub files: Vec<ItemFile>,
    /// The live link graph — edges + title lookup, live *and*
    /// archived items (same source `item show` uses).
    pub graph: Option<LiveGraph>,
    /// `handoffs/*.md` summaries.
    pub handoffs: Vec<HandoffRow>,
    /// The repo's kind table (names/pads for user kinds).
    pub kinds: KindTable,
    /// Per-source load problems — surfaced, never fatal.
    pub warnings: Vec<String>,
    /// Why there is no project at all.
    pub error: Option<String>,
}

impl Default for Snapshot {
    fn default() -> Self {
        Snapshot {
            project: None,
            ledger: None,
            files: Vec::new(),
            graph: None,
            handoffs: Vec::new(),
            kinds: myco_core::kind::prelude_table().clone(),
            warnings: Vec::new(),
            error: None,
        }
    }
}

impl Snapshot {
    /// The empty/degraded snapshot — `error` says why.
    fn failed(error: String) -> Self {
        Snapshot {
            error: Some(error),
            ..Snapshot::default()
        }
    }

    /// The `{"error": …}` payload every query emits when the
    /// snapshot couldn't load — the degradation marker pavo
    /// renderers already understand.
    pub fn error_payload(&self) -> serde_json::Value {
        serde_json::json!({"error": self.error.clone().unwrap_or_else(|| "no project".into())})
    }

    /// Is there a usable project?
    pub fn ok(&self) -> bool {
        self.error.is_none() && self.project.is_some()
    }
}

/// `params` → `(dir, root)` overrides. `dir` wins over `root`
/// when both are given (mirrors the CLI globals' precedence).
fn overrides(
    params: &serde_json::Map<String, serde_json::Value>,
) -> (Option<PathBuf>, Option<PathBuf>) {
    let dir = params
        .get("dir")
        .or_else(|| params.get("project"))
        .and_then(|v| v.as_str())
        .map(PathBuf::from);
    let root = params
        .get("root")
        .and_then(|v| v.as_str())
        .map(PathBuf::from);
    (dir, root)
}

/// Resolve + load. `params` are the wire op's caller params —
/// `dir`/`root`/`project` steer discovery, the rest pass through
/// to the query resolvers untouched.
pub fn load(params: &serde_json::Map<String, serde_json::Value>) -> Snapshot {
    let (mut dir, root) = overrides(params);
    if dir.is_none() && root.is_none() {
        if let Ok(d) = std::env::var("MYCO_DIR") {
            if !d.is_empty() {
                dir = Some(PathBuf::from(d));
            }
        }
    }
    let dir = dir.as_deref();
    let root = root.as_deref();

    let (proj, locked) = match items::open_read_with(dir, root, CONFIG, store_at) {
        Ok(pair) => pair,
        Err(e) => return Snapshot::failed(e),
    };

    let mut snap = Snapshot {
        project: Some(proj),
        kinds: locked.kinds().clone(),
        ..Snapshot::default()
    };

    // Ledger docs — a corrupt TODO.md/BACKLOG.md degrades the
    // board, not the whole snapshot.
    match items::load_ledger(&locked) {
        Ok(ledger) => snap.ledger = Some(ledger),
        Err(e) => snap.warnings.push(format!("ledger: {e}")),
    }

    // File-backed items, live scope only — `archive/` stays out of
    // listings (the TUI's FILES board didn't show it either); the
    // graph still sees archived edges via `live_graph`.
    for spec in locked.kinds().file_backed() {
        match locked.list_items(spec.id_kind()) {
            Ok(mut v) => snap.files.append(&mut v),
            Err(e) => snap
                .warnings
                .push(format!("{} files: {e}", spec.dir_name())),
        }
    }

    // Handoff files — id from the filename, latest entry for the
    // table's `updated`/`actor`/`note` columns.
    match locked.list_handoff_files() {
        Ok(hs) => {
            for h in hs {
                let mut row = HandoffRow {
                    id: h.id,
                    entries: 0,
                    archived: h.archived,
                    updated: None,
                    actor: None,
                    note: None,
                };
                match locked.read_handoffs(h.id) {
                    Ok(entries) => {
                        row.entries = entries.len();
                        if let Some(last) = entries.last() {
                            row.updated = Some(last.date.to_string());
                            row.actor = Some(last.actor.to_string());
                            let note: String = last.note.chars().take(120).collect();
                            row.note = Some(if last.note.chars().count() > 120 {
                                format!("{note}…")
                            } else {
                                note
                            });
                        }
                    }
                    Err(e) => snap.warnings.push(format!("handoffs/{}: {e}", h.id)),
                }
                snap.handoffs.push(row);
            }
        }
        Err(e) => snap.warnings.push(format!("handoffs: {e}")),
    }

    // The link graph — live read, no index (see `links::live_graph`
    // for why display surfaces derive rather than persist).
    let (graph, gw) = links::live_graph(&locked);
    snap.graph = Some(graph);
    snap.warnings.extend(gw);

    snap
}
