//! The TUI model — boards, cursor, modes, and the ledger ops every
//! key maps to. Mirrors pm's design: two board tabs over
//! `myco_ops::items::Ledger`, a flattened row list, and writes that
//! end in `items::commit`.
//!
//! Locking: the model holds an *unlocked* `MdStore` for reads. Every
//! mutation takes the exclusive worktree lock for just the
//! load→mutate→save→commit span (`mutate`), so `myco ui` never
//! blocks other myco invocations between keypresses — and the fresh
//! read under the lock means the TUI never saves a stale
//! startup-snapshot ledger over a concurrent writer's changes.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use myco_core::Ledger;
use myco_core::{
    eval_scoped, refs, Edge, EdgeKind, Item, ItemFile, ItemId, ItemKind, Scoped, Status,
    ViewFormat, ViewSource, ViewSpec,
};
use myco_ops::{discover, items, ops};
use myco_store::Store;
use myco_store_md::MdStore;
use ratatui::layout::Rect;

/// `MdStore` rooted at a worktree path — the TUI names the impl
/// (`myco-ops` is generic over [`Store`]; each front-end wires
/// its own store factory).
fn store_at(worktree: &Path, kinds: &myco_core::KindTable) -> Result<MdStore, String> {
    let root = camino::Utf8PathBuf::from_path_buf(worktree.to_path_buf())
        .map_err(|p| format!("{}: non-utf8 path", p.display()))?;
    Ok(MdStore::with_kinds(root, kinds.clone()))
}

/// Layered TOML — `Some(path)` pins the repo layer, `None` runs
/// the impl's default search. Same story as `store_at`: ops keeps
/// the *which path* policy, the front-end names the format.
fn config_layered(path: Option<&Path>) -> Result<myco_core::Config, String> {
    use myco_config::ConfigSource;
    myco_config_toml::TomlSource {
        path: path.map(|p| p.to_string_lossy().into_owned()),
    }
    .load()
}

/// The config impl ops calls share.
const CONFIG: myco_ops::config::ConfigLoader = myco_ops::config::ConfigLoader {
    layered: config_layered,
    file: myco_config_toml::load_file,
};

/// Which ledger doc the board tab shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Board {
    /// `TODO.md` — committed work.
    Todo,
    /// `BACKLOG.md` — deferred items.
    Backlog,
    /// File-backed kinds — `issues/`/`proposals/`/`plans/`/`adrs/`,
    /// grouped under kind headers. Read-only for now: task verbs
    /// (`done`/`defer`/…) don't apply here.
    Files,
}

impl Board {
    /// Tab label.
    pub fn label(self) -> &'static str {
        match self {
            Board::Todo => "TODO",
            Board::Backlog => "BACKLOG",
            Board::Files => "FILES",
        }
    }

    /// Tab order for `h`/`l` and the `1`/`2`/`3` jumps.
    pub const ALL: [Board; 3] = [Board::Todo, Board::Backlog, Board::Files];
}

/// UI mode — like pm's: the board handles keys differently per mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Browsing.
    Normal,
    /// Full-pane item preview (`Enter`, `esc` back).
    Preview,
    /// `a` — typing a new item title; `Enter` commits, `esc` drops.
    Add,
    /// `/` — live filter over the current board's items; `Enter`
    /// keeps the filter, `esc` clears it.
    Filter,
    /// `:` — ex-style command line (`:q` `:w` `:e` `:<id>` `:go <id>`
    /// `:set id`); `Enter` runs it, `esc` drops the line.
    Command,
    /// `?` — keybind overlay; `esc`/`?`/`q` closes.
    Help,
}

/// One registered repo's slice of the index universe — what an
/// index-sourced view (`--view org-*`) loads instead of the repo
/// store. `worktree` is where a mutation on this repo's items
/// opens its own store.
#[derive(Debug, Clone)]
pub struct OrgRepo {
    /// Registry name — the `repo:` row prefix.
    pub name: String,
    /// The registered `project/` worktree.
    pub worktree: PathBuf,
    /// Top-level items in the ledger (any status, live + archived).
    pub items: Vec<Item>,
    /// Ids that live under `archive/` — filtered out of rows.
    pub archived: BTreeSet<ItemId>,
    /// Rescan/load error, when the fresh snapshot couldn't be had
    /// (the last-good items still load).
    pub error: Option<String>,
}

/// Loads the index universe — injected by the composition root
/// (`myco ui` wires the redb index here; the TUI itself is
/// impl-free). Called at open and on every reload (`:w`/`R`).
pub type OrgLoader = Box<dyn FnMut() -> Result<Vec<OrgRepo>, String>>;

/// A flattened display row — section headers aren't selectable.
#[derive(Debug, Clone)]
pub enum Row {
    /// `## <section>` header line.
    Header(String),
    /// An item line; `depth` indents children under their parent,
    /// `repo` tags index-sourced items with their owning repo.
    Item {
        item: Box<Item>,
        depth: usize,
        repo: Option<String>,
    },
}

/// The whole TUI state.
pub struct Model {
    /// The repo `myco ui` launched in — `None` when discovery
    /// found no project (only index-sourced views open then).
    proj: Option<discover::Project>,
    /// Unlocked read handle on the launch repo. Mutations don't
    /// go through this field alone — `mutate` opens the target
    /// repo's store under `ops::lock_worktree` + a fresh
    /// `load_ledger`, so writes are serialized and never stale.
    /// `None` in index mode or when discovery failed.
    store: Option<MdStore>,
    ledger: Ledger,
    /// The index universe — `Some` when the resolved view is
    /// `source = "index"`: rows, links, and mutations address
    /// repo slices, not `store`/`ledger`.
    orgs: Option<Vec<OrgRepo>>,
    /// Reloads `orgs` (`:w`/`R`, post-editor).
    org_loader: Option<OrgLoader>,
    no_commit: bool,
    /// Active tab.
    pub board: Board,
    /// Display rows for `board`, rebuilt on reload/switch.
    pub rows: Vec<Row>,
    /// Index into `rows` (always on an `Item` row when any exist).
    pub sel: usize,
    /// Current mode.
    pub mode: Mode,
    /// `a` input buffer.
    pub input: String,
    /// `/` live filter — narrows visible item rows when non-empty.
    pub filter: String,
    /// `--view <name>` spec — narrows every board's item source
    /// (the filter only; group/sort/format are renderer concerns).
    pub view: Option<ViewSpec>,
    /// `:` command-line buffer — separate from `input` so an `a`
    /// draft isn't clobbered.
    pub cmdline: String,
    /// Item rows render their `T-NNN` id span — `:set id` /
    /// `:set noid` (vim aliases `numbers`/`nonumbers` accepted).
    pub show_ids: bool,
    /// Status-bar message (last action / error).
    pub status: String,
    /// Editor to open on the next loop turn (`E`).
    pub pending_editor: Option<PathBuf>,
    /// File-backed items for the `Files` board.
    files: Vec<ItemFile>,
    /// `handoffs/<ID>.md` files → `(id, joined note text)` — the
    /// link graph's handoff input, refreshed with `load_files`.
    handoffs: Vec<(ItemId, String)>,
    /// The in-memory link graph (P-005 phase 1): every edge the
    /// loaded ledger/files/handoffs declare. Rebuilt with the
    /// rows — O(items) once per rebuild, no index involved.
    links: Vec<Edge>,
    /// Filesystem fingerprint of the watched trees — `tick`
    /// compares against it each poll cycle. Refreshed by `reload`
    /// and by successful mutations, so the board's own writes
    /// don't retrigger a reload.
    watch_fp: u64,
    /// Preview pane scroll offset (`j`/`k`/`PgUp`/`PgDn`/`g`/`G`
    /// in Preview mode; clamped to the content at draw time).
    pub preview_scroll: u16,
    /// Multi-key prefix: `g` armed for `gg`, `d` for `dd`.
    pending: Option<char>,
    /// Board-list rect from the last frame — `render::draw` writes
    /// it so mouse clicks and wheel hits map to rows.
    pub board_area: Rect,
    /// First row index visible in `board_area` — the scroll offset
    /// the `List` widget chose to keep `sel` on screen.
    pub viewport_top: usize,
    /// Tab-bar rect when the frame is tall enough to draw one —
    /// for click-to-switch hit-testing.
    pub tabs_area: Option<Rect>,
}

impl Model {
    /// Discovery + first load. `dir`/`root` override like the CLI
    /// globals; `view` names a declarative view (builtin preset or
    /// `[views.<name>]` config). A `source = "index"` spec (`org-*`)
    /// swaps the repo store for the index universe via `org_loader`
    /// — and opens even when discovery found no project.
    pub fn open(
        dir: Option<&Path>,
        root: Option<&Path>,
        no_commit: bool,
        view: Option<String>,
        org_loader: Option<OrgLoader>,
    ) -> Result<Self, String> {
        // The view resolves first — its source picks the universe.
        // Discovery here is only for the config root, so a repo-less
        // launch still reaches an index-sourced spec.
        let proj = discover::discover(dir, root, CONFIG).ok();
        let cfg_root = proj
            .as_ref()
            .map(|p| p.root.clone())
            .or_else(|| root.map(Path::to_path_buf))
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("."));
        let view_spec = match &view {
            Some(name) => Some(myco_ops::view::resolve(&cfg_root, name, CONFIG)?.0.spec),
            None => None,
        };
        let index_sourced = view_spec
            .as_ref()
            .is_some_and(|v| v.source == ViewSource::Index);
        let mut org_loader = org_loader;
        let (proj, store, ledger, orgs) = if index_sourced {
            let Some(loader) = org_loader.as_mut() else {
                return Err("index-sourced view — this front-end has no index".into());
            };
            let orgs = loader()?;
            (
                proj,
                None,
                Ledger::new(
                    myco_core::TodoDoc::new(""),
                    myco_core::BacklogDoc::default(),
                ),
                Some(orgs),
            )
        } else {
            // Shared read lock for the initial load only — a live
            // writer fails us fast rather than rendering
            // mid-mutation. It drops with this scope: holding any
            // lock for the session would block every other myco
            // invocation while the board is open.
            let (proj, ledger) = {
                let (p, store) = items::open_read_with(dir, root, CONFIG, store_at)?;
                (p, items::load_ledger(&store)?)
            };
            let store = store_at(&proj.worktree, myco_core::kind::prelude_table())?;
            (Some(proj), Some(store), ledger, None)
        };
        let mut m = Model {
            proj,
            store,
            ledger,
            orgs,
            org_loader,
            no_commit,
            board: Board::Todo,
            rows: Vec::new(),
            sel: 0,
            mode: Mode::Normal,
            input: String::new(),
            filter: String::new(),
            view: view_spec,
            cmdline: String::new(),
            show_ids: true,
            status: String::new(),
            pending_editor: None,
            files: Vec::new(),
            handoffs: Vec::new(),
            links: Vec::new(),
            watch_fp: 0,
            preview_scroll: 0,
            pending: None,
            board_area: Rect::default(),
            viewport_top: 0,
            tabs_area: None,
        };
        m.load_files();
        m.rebuild_rows();
        m.restore_session();
        m.watch_fp = m.fingerprint();
        if let Some(name) = &view {
            m.status = format!("view: {name}");
        }
        match &m.orgs {
            Some(orgs) if orgs.is_empty() => {
                m.status = "index: no repos registered — `myco index add <path>`".into();
            }
            Some(orgs) => {
                let errs: Vec<String> = orgs
                    .iter()
                    .filter_map(|o| o.error.as_ref().map(|e| format!("{}: {e}", o.name)))
                    .collect();
                if !errs.is_empty() {
                    m.status = format!("index: {}", errs.join("; "));
                }
            }
            None => {}
        }
        Ok(m)
    }

    /// The kind table the open store resolves against — the
    /// prelude when nothing's loaded.
    fn kinds(&self) -> &myco_core::KindTable {
        match &self.store {
            Some(s) => s.kinds(),
            None => myco_core::kind::prelude_table(),
        }
    }

    /// `--view` narrows the item source — top-level only; children
    /// ride with a matching parent.
    fn view_matches(&self, item: &Item) -> bool {
        self.view
            .as_ref()
            .is_none_or(|v| v.filter.matches(item, self.kinds()))
    }

    /// Pull file-backed items for the `Files` board. Per-kind read
    /// errors land in the status bar — a broken item file degrades
    /// the board instead of vanishing silently. Index boards skip
    /// this — their `Files` tab reads snapshot metas, not files.
    pub(crate) fn load_files(&mut self) {
        let Some(store) = &self.store else {
            return;
        };
        self.files.clear();
        let mut errs = Vec::new();
        for kind in [
            ItemKind::Issue,
            ItemKind::Proposal,
            ItemKind::Plan,
            ItemKind::Adr,
        ] {
            match store.list_items(kind) {
                Ok(mut v) => self.files.append(&mut v),
                Err(e) => errs.push(format!(
                    "{}: {e}",
                    store
                        .kinds()
                        .spec_of(kind)
                        .map(|s| s.dir_name())
                        .unwrap_or_else(|| "?".into())
                )),
            }
        }
        // handoff files feed the link graph — the id comes from
        // the filename, the mention input from the entry notes
        self.handoffs.clear();
        match store.list_handoff_files() {
            Ok(hs) => {
                for h in hs {
                    match store.read_handoffs(h.id) {
                        Ok(entries) => self.handoffs.push((
                            h.id,
                            entries
                                .iter()
                                .map(|e| e.note.as_str())
                                .collect::<Vec<_>>()
                                .join("\n"),
                        )),
                        Err(e) => errs.push(format!("handoffs/{}: {e}", h.id)),
                    }
                }
            }
            Err(e) => errs.push(format!("handoffs: {e}")),
        }
        if !errs.is_empty() {
            self.status = format!("files: {}", errs.join("; "));
        }
    }

    /// Every loaded top-level item — the ledger docs in repo mode,
    /// the unioned repo slices on an index board. Children stay
    /// nested under their parents (callers flatten with
    /// `Item::subtree`).
    fn top_items(&self) -> Box<dyn Iterator<Item = &Item> + '_> {
        match &self.orgs {
            Some(orgs) => Box::new(orgs.iter().flat_map(|o| o.items.iter())),
            None => Box::new(self.ledger.todo.items().chain(self.ledger.backlog.items())),
        }
    }

    /// Rebuild the in-memory edge set from what's loaded — ledger
    /// item titles/metas, file bodies/metas, handoff files. Pure:
    /// no store hits, so `rebuild_rows` can call it freely.
    fn rebuild_links(&mut self) {
        let mut edges = std::collections::BTreeSet::new();
        for i in self.top_items().flat_map(Item::subtree) {
            edges.extend(refs::extract(&i.title, Some(i), i.id, self.kinds()));
        }
        for f in &self.files {
            edges.extend(refs::extract(
                &f.body,
                Some(&f.meta),
                f.meta.id,
                self.kinds(),
            ));
        }
        for (id, notes) in &self.handoffs {
            edges.insert(Edge::handoff(*id));
            edges.extend(refs::extract(notes, None, *id, self.kinds()));
        }
        self.links = edges.into_iter().collect();
    }

    /// Everything attached to `id` — `(kind, other)` pairs with
    /// incoming kinds already mirrored (`blocked_by`, …).
    /// The preview's related section renders this.
    pub fn related(&self, id: ItemId) -> Vec<(EdgeKind, ItemId)> {
        let mut out = Vec::new();
        for e in &self.links {
            if e.src == id {
                out.push((e.kind, e.dst));
            } else if e.dst == id {
                out.push((e.kind.mirror(), e.src));
            }
        }
        out.sort();
        out.dedup();
        out
    }

    /// Title for `id` when it's loaded — the related list
    /// annotates neighbors with it. On an index board the first
    /// repo's match wins (ids aren't unique across repos).
    pub fn item_title(&self, id: ItemId) -> Option<&str> {
        if let Some(f) = self.files.iter().find(|f| f.meta.id == id) {
            return Some(f.meta.title.as_str());
        }
        self.top_items()
            .flat_map(Item::subtree)
            .find(|i| i.id == id)
            .map(|i| i.title.as_str())
    }

    /// Body of a file-backed item (preview on the `Files` board).
    /// Repo mode reads the loaded `files`; an index board reads
    /// the file lazily through the owning repo's store.
    pub fn file_body(&self, id: ItemId) -> Option<String> {
        if let Some(orgs) = &self.orgs {
            let repo = self.selected_repo()?;
            let org = orgs.iter().find(|o| o.name == repo)?;
            return store_at(&org.worktree, myco_core::kind::prelude_table())
                .ok()
                .and_then(|s| s.item(id).ok().map(|f| f.body));
        }
        self.files
            .iter()
            .find(|f| f.meta.id == id)
            .map(|f| f.body.clone())
    }

    /// Reload the ledger docs *and* the file-backed items from disk
    /// (`R`, post-editor); an index board re-runs the loader —
    /// `load_fresh` rescans every repo whose project HEAD moved.
    /// The cursor tries to stay on the same id.
    pub fn reload(&mut self) {
        let sel_id = self.selected_id();
        let sel_repo = self.selected_repo().map(str::to_owned);
        let at = self.sel;
        if let Some(orgs) = &mut self.orgs {
            if let Some(loader) = &mut self.org_loader {
                match loader() {
                    Ok(fresh) => *orgs = fresh,
                    Err(e) => self.status = format!("reload: {e}"),
                }
            }
        } else if let Some(store) = &self.store {
            match items::load_ledger(store) {
                Ok(ledger) => self.ledger = ledger,
                Err(e) => self.status = format!("reload: {e}"),
            }
        }
        self.load_files();
        self.rebuild_rows();
        self.reselect(sel_id, sel_repo.as_deref(), at);
        self.watch_fp = self.fingerprint();
    }

    /// Event-loop heartbeat — reload when the watched trees changed
    /// on disk: another `myco` invocation, a `git pull`/checkout in
    /// the worktree, a hand-edit committed elsewhere, or (index
    /// boards) registry/snapshot writes under the data dir. The
    /// scan hashes mtimes+sizes of the small markdown trees, so it
    /// runs every poll cycle.
    pub fn tick(&mut self) {
        if self.fingerprint() != self.watch_fp {
            self.reload();
        }
    }

    /// Hash of everything the board reads: the `project/` worktree
    /// in repo mode; the index data dir (`repos.toml`,
    /// `index.redb`) plus every repo's worktree on an index board.
    fn fingerprint(&self) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        match &self.orgs {
            Some(orgs) => {
                tree_fp(&data_dir(), &mut h);
                for o in orgs {
                    tree_fp(&o.worktree, &mut h);
                }
            }
            None => {
                if let Some(p) = &self.proj {
                    tree_fp(&p.worktree, &mut h);
                }
            }
        }
        std::hash::Hasher::finish(&h)
    }

    /// Flatten the active board's sections into rows.
    fn rebuild_rows(&mut self) {
        self.rebuild_links();
        self.rows.clear();
        // take/restore: the org slices feed row building while
        // `&mut self` accumulates rows — no clone of the universe
        if self.orgs.is_some() {
            let orgs = self.orgs.take().unwrap_or_default();
            self.rebuild_org_rows(&orgs);
            self.orgs = Some(orgs);
        } else {
            match self.board {
                Board::Files => {
                    for (kind, label) in [
                        (ItemKind::Issue, "issues"),
                        (ItemKind::Proposal, "proposals"),
                        (ItemKind::Plan, "plans"),
                        (ItemKind::Adr, "adrs"),
                    ] {
                        let group: Vec<Item> = self
                            .files
                            .iter()
                            .filter(|f| f.meta.id.kind() == kind && self.view_matches(&f.meta))
                            .map(|f| f.meta.clone())
                            .collect();
                        if group.is_empty() {
                            continue;
                        }
                        self.rows.push(Row::Header(label.to_string()));
                        push_items(&group, 0, None, &mut self.rows);
                    }
                }
                _ => {
                    let sections = if self.board == Board::Todo {
                        &self.ledger.todo.sections
                    } else {
                        &self.ledger.backlog.sections
                    };
                    for s in sections {
                        if let Some(name) = &s.name {
                            self.rows.push(Row::Header(name.clone()));
                        }
                        let items: Vec<Item> = s
                            .items
                            .iter()
                            .filter(|i| self.view_matches(i))
                            .cloned()
                            .collect();
                        push_items(&items, 0, None, &mut self.rows);
                    }
                }
            }
        }
        self.apply_filter();
        self.sel = self.first_item_row().unwrap_or(0);
    }

    /// Index-mode rows: union the loaded repo slices for this tab,
    /// run the spec through `eval_scoped` (`filter → group → sort →
    /// limit`), flatten groups to rows. `format` pins to `list` so
    /// a board-format spec doesn't invent a status axis the row
    /// list has no columns for; `group_by` (e.g. `repo` on
    /// `org-status`) still yields `##` headers. Rescan failures
    /// surface as header lines above the groups.
    fn rebuild_org_rows(&mut self, orgs: &[OrgRepo]) {
        let board = self.board;
        // rescan errors are rows, not just status — they persist
        // past the first status-bar overwrite
        for o in orgs {
            if let Some(e) = &o.error {
                self.rows.push(Row::Header(format!("{}: {e}", o.name)));
            }
        }
        let scoped: Vec<Scoped> = orgs
            .iter()
            .flat_map(|o| {
                o.items
                    .iter()
                    .filter(|i| !o.archived.contains(&i.id) && tab_match(board, i))
                    .map(move |i| Scoped::tagged(o.name.as_str(), i))
            })
            .collect();
        let mut spec = self.view.clone().unwrap_or_default();
        spec.format = ViewFormat::List;
        let groups = eval_scoped(scoped, &spec, self.kinds());
        if self.board == Board::Files && spec.group_by.is_none() {
            // mirror the repo-mode FILES board — kind headers in
            // declaration order, eval's sort preserved within each
            let items: Vec<Scoped> = groups
                .iter()
                .flat_map(|g| g.items.iter().copied())
                .collect();
            for (kind, label) in [
                (ItemKind::Issue, "issues"),
                (ItemKind::Proposal, "proposals"),
                (ItemKind::Plan, "plans"),
                (ItemKind::Adr, "adrs"),
            ] {
                let group: Vec<Scoped> = items
                    .iter()
                    .copied()
                    .filter(|s| s.id.kind() == kind)
                    .collect();
                if group.is_empty() {
                    continue;
                }
                self.rows.push(Row::Header(label.to_string()));
                for s in group {
                    push_scoped(s, 0, &mut self.rows);
                }
            }
        } else {
            for g in &groups {
                if g.items.is_empty() {
                    continue;
                }
                if let Some(l) = &g.label {
                    self.rows.push(Row::Header(l.clone()));
                }
                for s in &g.items {
                    push_scoped(*s, 0, &mut self.rows);
                }
            }
        }
    }

    /// `/` filter — keep only item rows matching the query
    /// (title/id/labels/repo, case-insensitive); headers drop out.
    fn apply_filter(&mut self) {
        if self.filter.is_empty() {
            return;
        }
        let q = self.filter.to_lowercase();
        self.rows.retain(|r| {
            matches!(r, Row::Item { item, repo, .. }
                if item.title.to_lowercase().contains(&q)
                    || item.id.to_string().to_lowercase().contains(&q)
                    || repo.as_deref().is_some_and(|s| s.to_lowercase().contains(&q))
                    || item.labels.iter().any(|l| l.to_lowercase().contains(&q)))
        });
    }

    /// After a mutation/reload: land the cursor back on `id`, or on
    /// the item row nearest `from` — the removed/done item's old
    /// slot — rather than teleporting to the top of the board.
    /// `repo` scopes the id match to one repo slice — ids aren't
    /// unique across an index board.
    fn reselect(&mut self, id: Option<ItemId>, repo: Option<&str>, from: usize) {
        if let Some(id) = id {
            if let Some(i) = self.rows.iter().position(|r| {
                matches!(r, Row::Item { item, repo: r_repo, .. }
                    if item.id == id
                        && repo.is_none_or(|name| r_repo.as_deref() == Some(name)))
            }) {
                self.sel = i;
                return;
            }
        }
        self.sel = self.nearest_item_row(from).unwrap_or(0);
    }

    /// The item row nearest `from` — `from` itself when it's an
    /// item, else the next item row after it, else the last one
    /// before it.
    fn nearest_item_row(&self, from: usize) -> Option<usize> {
        if self.rows.is_empty() {
            return None;
        }
        let from = from.min(self.rows.len() - 1);
        if matches!(self.rows[from], Row::Item { .. }) {
            return Some(from);
        }
        match self.rows[from..]
            .iter()
            .position(|r| matches!(r, Row::Item { .. }))
        {
            Some(off) => Some(from + off),
            None => self.rows[..from]
                .iter()
                .rposition(|r| matches!(r, Row::Item { .. })),
        }
    }

    fn first_item_row(&self) -> Option<usize> {
        self.rows.iter().position(|r| matches!(r, Row::Item { .. }))
    }

    /// The id under the cursor.
    fn selected_id(&self) -> Option<ItemId> {
        self.selected().map(|i| i.id)
    }

    /// The item under the cursor.
    pub fn selected(&self) -> Option<&Item> {
        match self.rows.get(self.sel) {
            Some(Row::Item { item, .. }) => Some(item),
            _ => None,
        }
    }

    /// The repo tag on the row under the cursor — `Some` only on
    /// index-sourced boards.
    pub fn selected_repo(&self) -> Option<&str> {
        match self.rows.get(self.sel) {
            Some(Row::Item { repo, .. }) => repo.as_deref(),
            _ => None,
        }
    }

    /// The file `E` opens — the board doc, or the selected item's
    /// file on the `Files` board (`None` when nothing is selected
    /// there — `E` on an empty Files board has no target). On an
    /// index board the selected row's owning repo supplies the
    /// worktree.
    fn board_path(&self) -> Option<PathBuf> {
        if let Some(orgs) = &self.orgs {
            let repo = self.selected_repo()?;
            let wt = &orgs.iter().find(|o| o.name == repo)?.worktree;
            return match self.board {
                Board::Todo => Some(wt.join("TODO.md")),
                Board::Backlog => Some(wt.join("BACKLOG.md")),
                Board::Files => self
                    .selected()
                    .and_then(|i| {
                        store_at(wt, myco_core::kind::prelude_table())
                            .and_then(|s| items::item_rel_path(&s, i))
                            .ok()
                    })
                    .map(|p| wt.join(p)),
            };
        }
        let proj = self.proj.as_ref()?;
        match self.board {
            Board::Todo => Some(proj.worktree.join("TODO.md")),
            Board::Backlog => Some(proj.worktree.join("BACKLOG.md")),
            Board::Files => self
                .selected()
                .and_then(|i| {
                    self.store
                        .as_ref()
                        .and_then(|s| items::item_rel_path(s, i).ok())
                })
                .map(|p| proj.worktree.join(p)),
        }
    }

    /// `E` / `:e` — open the board doc (or the file item under the
    /// cursor) in `$EDITOR`.
    fn open_editor(&mut self) {
        match self.board_path() {
            Some(p) => self.pending_editor = Some(p),
            None => self.status = "nothing to edit — select a file-backed item".into(),
        }
    }

    /// Handle one key. Returns `true` to quit — key errors land in
    /// the status bar instead of killing the app.
    pub fn key(&mut self, k: KeyEvent) -> bool {
        // Ctrl-C is the force-quit and must work from every mode —
        // even mid-`a`, mid-`:` or under an overlay.
        if k.code == KeyCode::Char('c') && k.modifiers.contains(KeyModifiers::CONTROL) {
            return true;
        }
        match self.mode {
            Mode::Preview => return self.key_preview(k),
            Mode::Add => return self.key_add(k),
            Mode::Filter => return self.key_filter(k),
            Mode::Command => return self.key_command(k),
            Mode::Help => return self.key_help(k),
            Mode::Normal => {}
        }
        let alt = k.modifiers.contains(KeyModifiers::ALT);
        let was_g = self.pending == Some('g');
        let was_d = self.pending == Some('d');
        self.pending = None;
        match k.code {
            KeyCode::Char('q') => return true,
            KeyCode::Char('1') => self.switch_board(0),
            KeyCode::Char('2') => self.switch_board(1),
            KeyCode::Char('3') => self.switch_board(2),
            KeyCode::Char('h') | KeyCode::Left => self.step_board(-1),
            KeyCode::Char('l') | KeyCode::Right => self.step_board(1),
            KeyCode::Char('j') | KeyCode::Down if alt => self.mutate(|m| m.reorder(1)),
            KeyCode::Char('k') | KeyCode::Up if alt => self.mutate(|m| m.reorder(-1)),
            KeyCode::Char('j') | KeyCode::Down => self.move_sel(1),
            KeyCode::Char('k') | KeyCode::Up => self.move_sel(-1),
            KeyCode::Char('g') if was_g => self.sel = self.first_item_row().unwrap_or(0),
            KeyCode::Char('g') => self.pending = Some('g'),
            KeyCode::Char('G') => self.sel = self.last_item_row().unwrap_or(0),
            KeyCode::Char('{') => self.jump_section(-1),
            KeyCode::Char('}') => self.jump_section(1),
            KeyCode::Char('d') if was_d => self.mutate(|m| m.done_selected()),
            KeyCode::Char('d') => self.pending = Some('d'),
            KeyCode::Char('D') => self.mutate(|m| m.defer_selected()),
            KeyCode::Char('p') => self.mutate(|m| m.promote_selected()),
            KeyCode::Char(' ') => self.mutate(|m| m.done_selected()),
            KeyCode::Char('a') => {
                if self.orgs.is_some() {
                    self.status = "add writes one repo — run `myco ui` inside it".into();
                } else if self.board == Board::Files {
                    self.status = "add is a TODO/BACKLOG op — `myco <kind> new` for files".into();
                } else {
                    self.mode = Mode::Add;
                    self.input.clear();
                }
            }
            KeyCode::Char('/') => self.mode = Mode::Filter,
            KeyCode::Char(':') => {
                self.mode = Mode::Command;
                self.cmdline.clear();
            }
            KeyCode::Char('F') => {
                self.filter.clear();
                self.rebuild_rows();
            }
            KeyCode::Enter => {
                if self.selected().is_some() {
                    self.mode = Mode::Preview;
                    self.preview_scroll = 0;
                }
            }
            KeyCode::Char('E') => self.open_editor(),
            KeyCode::Char('R') => self.reload(),
            KeyCode::Char('?') => self.mode = Mode::Help,
            _ => {}
        }
        false
    }

    /// The one mutation entry point: the `Files` board is read-only
    /// (task verbs don't apply — `myco <kind>` edits file items),
    /// and every error lands in the status bar followed by a resync
    /// from disk, so a failed verb never kills the session.
    fn mutate(&mut self, f: impl FnOnce(&mut Self) -> Result<(), String>) {
        if self.board == Board::Files {
            self.status = "FILES is read-only — task verbs apply to TODO/BACKLOG".into();
            return;
        }
        match f(self) {
            // the write landed — refresh the watch fingerprint so
            // `tick` doesn't fire a redundant reload for our own
            // commit
            Ok(()) => self.watch_fp = self.fingerprint(),
            Err(e) => {
                // resync — the write may have partially landed, or
                // another writer moved the rows under us
                self.reload();
                self.status = e;
            }
        }
    }

    /// The `(repo, worktree, store)` a mutation writes through —
    /// the launch repo's worktree in repo mode; the selected row's
    /// owning repo on an index board (`repo:ID` is the tag). The
    /// store is fresh every call, so a long-lived board never holds
    /// a stale handle.
    fn target(&self) -> Result<(Option<String>, PathBuf, MdStore), String> {
        if let Some(orgs) = &self.orgs {
            let name = self
                .selected_repo()
                .ok_or_else(|| "no repo item under the cursor".to_string())?;
            let org = orgs
                .iter()
                .find(|o| o.name == name)
                .ok_or_else(|| format!("{name}: repo not in the index"))?;
            let wt = org.worktree.clone();
            Ok((
                Some(name.to_string()),
                wt.clone(),
                store_at(&wt, myco_core::kind::prelude_table())?,
            ))
        } else {
            let proj = self
                .proj
                .as_ref()
                .ok_or_else(|| "no project worktree".to_string())?;
            Ok((
                None,
                proj.worktree.clone(),
                store_at(&proj.worktree, myco_core::kind::prelude_table())?,
            ))
        }
    }

    /// Swap in the ledger a mutation just saved — `self.ledger`
    /// wholesale in repo mode; on an index board only the owning
    /// repo's task slice changes (file-kind items aren't ledger
    /// docs, so they ride untouched).
    fn apply_ledger(&mut self, repo: Option<&str>, ledger: Ledger) {
        match (repo, self.orgs.as_mut()) {
            (Some(name), Some(orgs)) => {
                if let Some(o) = orgs.iter_mut().find(|o| o.name == name) {
                    let mut items: Vec<Item> = ledger
                        .todo
                        .sections
                        .iter()
                        .chain(&ledger.backlog.sections)
                        .flat_map(|s| s.items.clone())
                        .collect();
                    items.extend(
                        o.items
                            .iter()
                            .filter(|i| i.id.kind() != ItemKind::Task)
                            .cloned(),
                    );
                    o.items = items;
                }
            }
            _ => self.ledger = ledger,
        }
    }

    /// Preview mode: `esc`/`enter`/`q` back to the board;
    /// `j`/`k`/`PgUp`/`PgDn`/`g`/`G` scroll the detail pane.
    fn key_preview(&mut self, k: KeyEvent) -> bool {
        match k.code {
            KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') => self.mode = Mode::Normal,
            KeyCode::Char('j') | KeyCode::Down => {
                self.preview_scroll = self.preview_scroll.saturating_add(1);
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.preview_scroll = self.preview_scroll.saturating_sub(1);
            }
            KeyCode::PageDown => {
                self.preview_scroll = self.preview_scroll.saturating_add(10);
            }
            KeyCode::PageUp => {
                self.preview_scroll = self.preview_scroll.saturating_sub(10);
            }
            KeyCode::Char('g') | KeyCode::Home => self.preview_scroll = 0,
            KeyCode::Char('G') | KeyCode::End => self.preview_scroll = u16::MAX,
            _ => {}
        }
        false
    }

    /// `/` filter — chars narrow rows live; `Enter` keeps the
    /// filter set, `esc` clears it.
    fn key_filter(&mut self, k: KeyEvent) -> bool {
        match k.code {
            KeyCode::Esc => {
                self.filter.clear();
                self.mode = Mode::Normal;
                self.rebuild_rows();
            }
            KeyCode::Enter => self.mode = Mode::Normal,
            KeyCode::Backspace => {
                self.filter.pop();
                self.rebuild_rows();
            }
            // plain chars only — modifier chords (ctrl+d, alt+x)
            // aren't filter text
            KeyCode::Char(c) if k.modifiers.is_empty() || k.modifiers == KeyModifiers::SHIFT => {
                self.filter.push(c);
                self.rebuild_rows();
            }
            _ => {}
        }
        false
    }

    /// `:` command line — chars append, `Enter` runs `exec_command`,
    /// `esc` drops the line.
    fn key_command(&mut self, k: KeyEvent) -> bool {
        match k.code {
            KeyCode::Esc => self.mode = Mode::Normal,
            KeyCode::Enter => {
                let line = std::mem::take(&mut self.cmdline);
                self.mode = Mode::Normal;
                return self.exec_command(&line);
            }
            KeyCode::Backspace => {
                self.cmdline.pop();
            }
            KeyCode::Char(c) if k.modifiers.is_empty() || k.modifiers == KeyModifiers::SHIFT => {
                self.cmdline.push(c);
            }
            _ => {}
        }
        false
    }

    /// Run one `:` line — first word is the command, the rest its
    /// args. v1 keeps the set small: `q`/`q!` quit, `w` reloads
    /// from disk, `e` opens the board doc in `$EDITOR`, `go <id>`
    /// and a bare `<id>` jump the cursor, `set id`/`noid` toggles
    /// the id column (`numbers`/`nonumbers` also accepted).
    fn exec_command(&mut self, line: &str) -> bool {
        let line = line.trim();
        let mut parts = line.splitn(2, char::is_whitespace);
        let cmd = parts.next().unwrap_or("");
        let arg = parts.next().unwrap_or("").trim();
        match cmd {
            "" => {}
            "q" | "q!" => return true,
            "w" => self.reload(),
            "e" => self.open_editor(),
            "go" => match arg.parse::<ItemId>() {
                Ok(id) => self.jump_to(id),
                Err(_) => self.status = format!(":go expects an id, got {arg:?}"),
            },
            "set" => match arg {
                "id" | "numbers" => self.show_ids = true,
                "noid" | "nonumbers" => self.show_ids = false,
                _ => self.status = ":set takes id / noid".into(),
            },
            _ => match cmd.parse::<ItemId>() {
                Ok(id) => self.jump_to(id),
                Err(_) => self.status = format!("unknown command :{cmd}"),
            },
        }
        false
    }

    /// `:<id>` / `:go <id>` — land the cursor on the item when it's
    /// a row on this board.
    fn jump_to(&mut self, id: ItemId) {
        if self
            .rows
            .iter()
            .any(|r| matches!(r, Row::Item { item, .. } if item.id == id))
        {
            self.reselect(Some(id), None, self.sel);
        } else {
            self.status = format!("{id}: not on this board");
        }
    }

    /// Help is a dead end — every key but the closers is swallowed.
    fn key_help(&mut self, k: KeyEvent) -> bool {
        match k.code {
            KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q') => self.mode = Mode::Normal,
            _ => {}
        }
        false
    }

    fn key_add(&mut self, k: KeyEvent) -> bool {
        match k.code {
            KeyCode::Esc => self.mode = Mode::Normal,
            KeyCode::Enter => {
                let title = self.input.trim().to_string();
                self.mode = Mode::Normal;
                if !title.is_empty() {
                    self.mutate(|m| m.add_item(&title));
                }
            }
            KeyCode::Backspace => {
                self.input.pop();
            }
            KeyCode::Char(c) if k.modifiers.is_empty() || k.modifiers == KeyModifiers::SHIFT => {
                self.input.push(c);
            }
            _ => {}
        }
        false
    }

    /// Mouse input (Normal mode only — acting on clicks under an
    /// overlay or mid-`a`/`/` edit would be surprising): the wheel
    /// moves the cursor over the board list, a left click picks a
    /// board tab on the tab row or selects the item row under it.
    pub fn mouse(&mut self, ev: MouseEvent) {
        if self.mode != Mode::Normal {
            return;
        }
        let pos = (ev.column, ev.row).into();
        match ev.kind {
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                if self.board_area.contains(pos) {
                    let delta = if ev.kind == MouseEventKind::ScrollUp {
                        -1
                    } else {
                        1
                    };
                    self.move_sel(delta);
                }
            }
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some(tabs) = self.tabs_area {
                    if ev.row == tabs.y {
                        if let Some(i) =
                            crate::render::tab_hit(tabs.width, ev.column.saturating_sub(tabs.x))
                        {
                            self.switch_board(i);
                        }
                        return;
                    }
                }
                if self.board_area.contains(pos) {
                    let row = self.viewport_top + usize::from(ev.row - self.board_area.y);
                    self.select_row(row);
                }
            }
            _ => {}
        }
    }

    fn last_item_row(&self) -> Option<usize> {
        self.rows
            .iter()
            .rposition(|r| matches!(r, Row::Item { .. }))
    }

    /// j/k — skip header rows, wrap nothing (pm stops at edges).
    fn move_sel(&mut self, delta: isize) {
        let mut i = self.sel as isize;
        loop {
            i += delta;
            if i < 0 || i as usize >= self.rows.len() {
                break;
            }
            if matches!(self.rows[i as usize], Row::Item { .. }) {
                self.sel = i as usize;
                break;
            }
        }
    }

    /// `{`/`}` — jump between section boundaries (kind groups on
    /// the `Files` board, `##` sections on the ledgers). `}` lands
    /// on the first item under the next header; `{` on the first
    /// item of the current group, or the previous group's when
    /// already there. Edges stop, matching `move_sel`.
    fn jump_section(&mut self, delta: isize) {
        let is_item = |i: usize| matches!(self.rows.get(i), Some(Row::Item { .. }));
        let is_header = |r: &Row| matches!(r, Row::Header(_));
        let item_after = |h: usize| (h + 1..self.rows.len()).find(|&i| is_item(i));
        if delta > 0 {
            if let Some(h) = (self.sel + 1..self.rows.len()).find(|&i| is_header(&self.rows[i])) {
                if let Some(t) = item_after(h) {
                    self.sel = t;
                }
            }
            return;
        }
        match self.rows[..self.sel].iter().rposition(is_header) {
            // In a group: first item, else the previous group's first.
            Some(h) => match item_after(h) {
                Some(first) if first == self.sel => {
                    // Group start: previous group, or the leading
                    // pre-header span when no header precedes.
                    let prev = self.rows[..h]
                        .iter()
                        .rposition(is_header)
                        .and_then(item_after);
                    self.sel = prev.or_else(|| self.first_item_row()).unwrap_or(self.sel);
                }
                Some(t) => self.sel = t,
                None => {}
            },
            // Before any header — the leading span is the first group.
            None => {
                if let Some(t) = self.first_item_row() {
                    self.sel = t;
                }
            }
        }
    }

    /// Mouse click — land the cursor on `row`, but only on items:
    /// headers aren't selectable and misses keep `sel` put.
    pub fn select_row(&mut self, row: usize) {
        if matches!(self.rows.get(row), Some(Row::Item { .. })) {
            self.sel = row;
        }
    }

    fn switch_board(&mut self, idx: usize) {
        self.board = Board::ALL[idx];
        self.rebuild_rows();
    }

    fn step_board(&mut self, delta: isize) {
        let idx = (Board::ALL
            .iter()
            .position(|b| *b == self.board)
            .unwrap_or(0) as isize
            + delta)
            .rem_euclid(Board::ALL.len() as isize) as usize;
        self.switch_board(idx);
    }

    /// `Space`/`dd` — the `todo done` verb: walk to done, remove the
    /// line (and its subtree), one commit.
    fn done_selected(&mut self) -> Result<(), String> {
        let Some(id) = self.selected_id() else {
            return Ok(());
        };
        let (repo, wt, store) = self.target()?;
        let _lock = ops::lock_worktree(&wt)?;
        let mut ledger = items::load_ledger(&store)?;
        let loc = ledger
            .locate(id)
            .ok_or_else(|| format!("{id}: no such task"))?;
        if loc.backlog {
            self.status = format!("{id} is deferred — `p` promotes it first");
            return Ok(());
        }
        let mut item = ledger
            .take(&loc)
            .ok_or_else(|| format!("{id}: stale task location"))?;
        items::walk_to(&mut item, Status::Done)?;
        items::save_ledger(&store, &ledger)?;
        items::commit(&wt, &format!("chore: done {id}"), None, self.no_commit)?;
        self.apply_ledger(repo.as_deref(), ledger);
        self.status = format!("done {id} — {}", item.title);
        let at = self.sel;
        self.rebuild_rows();
        self.reselect(None, repo.as_deref(), at);
        Ok(())
    }

    /// `D` — `todo defer`: park the subtree into BACKLOG.md.
    fn defer_selected(&mut self) -> Result<(), String> {
        let Some(id) = self.selected_id() else {
            return Ok(());
        };
        let (repo, wt, store) = self.target()?;
        let _lock = ops::lock_worktree(&wt)?;
        let mut ledger = items::load_ledger(&store)?;
        let loc = ledger
            .locate(id)
            .ok_or_else(|| format!("{id}: no such task"))?;
        if loc.backlog {
            self.status = format!("{id} is already deferred");
            return Ok(());
        }
        let mut item = ledger
            .take(&loc)
            .ok_or_else(|| format!("{id}: stale task location"))?;
        items::walk_to(&mut item, Status::Deferred)?;
        items::walk_descendants_to(&mut item, Status::Deferred)?;
        ledger.put_backlog(item, None);
        items::save_ledger(&store, &ledger)?;
        items::commit(&wt, &format!("chore: defer {id}"), None, self.no_commit)?;
        self.apply_ledger(repo.as_deref(), ledger);
        self.status = format!("deferred {id} → BACKLOG.md");
        let at = self.sel;
        self.rebuild_rows();
        self.reselect(None, repo.as_deref(), at);
        Ok(())
    }

    /// `p` — `backlog promote`: unpark the subtree into TODO.md.
    fn promote_selected(&mut self) -> Result<(), String> {
        let Some(id) = self.selected_id() else {
            return Ok(());
        };
        let (repo, wt, store) = self.target()?;
        let _lock = ops::lock_worktree(&wt)?;
        let mut ledger = items::load_ledger(&store)?;
        let loc = ledger
            .locate(id)
            .ok_or_else(|| format!("{id}: no such task"))?;
        if !loc.backlog {
            self.status = format!("{id} isn't in BACKLOG.md");
            return Ok(());
        }
        let mut item = ledger
            .take(&loc)
            .ok_or_else(|| format!("{id}: stale task location"))?;
        items::walk_to(&mut item, Status::Open)?;
        items::walk_descendants_to(&mut item, Status::Open)?;
        ledger.put_todo(item, None);
        items::save_ledger(&store, &ledger)?;
        items::commit(&wt, &format!("chore: promote {id}"), None, self.no_commit)?;
        self.apply_ledger(repo.as_deref(), ledger);
        self.status = format!("promoted {id} → TODO.md");
        self.rebuild_rows();
        self.reselect(Some(id), repo.as_deref(), self.sel);
        Ok(())
    }

    /// `Alt+↑/↓` — sibling reorder (pm's backlog priority move).
    fn reorder(&mut self, delta: isize) -> Result<(), String> {
        let Some(id) = self.selected_id() else {
            return Ok(());
        };
        let (repo, wt, store) = self.target()?;
        let _lock = ops::lock_worktree(&wt)?;
        let mut ledger = items::load_ledger(&store)?;
        let Some(mut loc) = ledger.locate(id) else {
            return Err(format!("{id}: no such task"));
        };
        if !ledger.move_within(&mut loc, delta) {
            return Ok(());
        }
        items::save_ledger(&store, &ledger)?;
        items::commit(&wt, &format!("chore: reorder {id}"), None, self.no_commit)?;
        self.apply_ledger(repo.as_deref(), ledger);
        self.status = format!("reordered {id}");
        self.rebuild_rows();
        self.reselect(Some(id), repo.as_deref(), self.sel);
        Ok(())
    }

    /// `a` Enter — `todo add`/`backlog add` for the active board.
    /// Repo mode only — the `a` key refuses on index boards (the
    /// universe writes per repo; which repo is ambiguous there).
    fn add_item(&mut self, title: &str) -> Result<(), String> {
        let (repo, wt, store) = self.target()?;
        let _lock = ops::lock_worktree(&wt)?;
        let mut ledger = items::load_ledger(&store)?;
        let id = store.alloc_id(ItemKind::Task).map_err(|e| e.to_string())?;
        let item = match self.board {
            Board::Todo => Item::new(id, title).created(items::today()),
            _ => Item::new(id, title)
                .status(Status::Deferred)
                .created(items::today()),
        };
        match self.board {
            Board::Todo => ledger.put_todo(item.clone(), None),
            _ => ledger.put_backlog(item.clone(), None),
        }
        items::save_ledger(&store, &ledger)?;
        items::commit(&wt, &format!("chore: add {id}"), None, self.no_commit)?;
        self.apply_ledger(repo.as_deref(), ledger);
        self.status = format!("added {}", items::compact(&item));
        self.rebuild_rows();
        self.reselect(Some(id), repo.as_deref(), self.sel);
        Ok(())
    }
}

/// Hash `path` into `h` — metadata always, contents recursively
/// for dirs (entries sorted for a deterministic hash). `.git` is
/// skipped (a real `.git` dir's internals churn on unrelated git
/// ops); missing paths hash a marker so creation counts as a
/// change. Symlinks hash as files — never followed.
fn tree_fp(path: &Path, h: &mut std::collections::hash_map::DefaultHasher) {
    use std::hash::Hash;
    let Ok(md) = std::fs::symlink_metadata(path) else {
        path.as_os_str().hash(h);
        false.hash(h);
        return;
    };
    path.as_os_str().hash(h);
    md.is_dir().hash(h);
    md.modified().ok().hash(h);
    md.len().hash(h);
    if !md.is_dir() {
        return;
    }
    let Ok(rd) = std::fs::read_dir(path) else {
        return;
    };
    let mut entries: Vec<_> = rd.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        if e.file_name() == ".git" {
            continue;
        }
        tree_fp(&e.path(), h);
    }
}

/// `$MYCO_HOME` → `$XDG_DATA_HOME/myco` → `~/.local/share/myco` —
/// same convention as the index.
fn data_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("MYCO_HOME") {
        return PathBuf::from(d);
    }
    if let Some(d) = std::env::var_os("XDG_DATA_HOME") {
        return PathBuf::from(d).join("myco");
    }
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/share/myco")
}

/// Per-repo session file — `<data_dir>/ui/<hash-of-root>.toml`.
fn session_path(root: &Path) -> PathBuf {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    root.hash(&mut h);
    data_dir()
        .join("ui")
        .join(format!("{:016x}.toml", h.finish()))
}

impl Model {
    /// What the session file is keyed on — the repo root in repo
    /// mode; all index boards share one `index` key (the launch
    /// repo doesn't own the view).
    fn session_key(&self) -> PathBuf {
        match &self.orgs {
            Some(_) => PathBuf::from("index"),
            None => self
                .proj
                .as_ref()
                .map(|p| p.root.clone())
                .unwrap_or_else(|| PathBuf::from("index")),
        }
    }

    /// Persist board + selected item for the next launch (`q`).
    pub fn save_session(&self) {
        let board = match self.board {
            Board::Todo => "todo",
            Board::Backlog => "backlog",
            Board::Files => "files",
        };
        let item = self
            .selected_id()
            .map(|i| i.to_string())
            .unwrap_or_default();
        let p = session_path(&self.session_key());
        let _ = std::fs::create_dir_all(p.parent().unwrap_or(p.as_path()));
        let _ = std::fs::write(p, format!("board = \"{board}\"\nitem = \"{item}\"\n"));
    }

    /// Reopen on the last board + item (best effort).
    fn restore_session(&mut self) {
        let Ok(src) = std::fs::read_to_string(session_path(&self.session_key())) else {
            return;
        };
        let get = |key: &str| {
            src.lines()
                .find_map(|l| l.split_once('=').filter(|(k, _)| k.trim() == key))
                .map(|(_, v)| v.trim().trim_matches('"').to_string())
        };
        self.board = match get("board").as_deref() {
            Some("backlog") => Board::Backlog,
            Some("files") => Board::Files,
            _ => Board::Todo,
        };
        self.rebuild_rows();
        if let Some(id) = get("item").and_then(|s| s.parse::<ItemId>().ok()) {
            self.reselect(Some(id), None, 0);
        }
    }
}

/// Which tab an index-universe item lands on — tasks split
/// live/deferred onto TODO/BACKLOG; file kinds go to FILES.
fn tab_match(board: Board, i: &Item) -> bool {
    match board {
        Board::Todo => i.id.kind() == ItemKind::Task && i.status != Status::Deferred,
        Board::Backlog => i.id.kind() == ItemKind::Task && i.status == Status::Deferred,
        Board::Files => i.id.kind() != ItemKind::Task,
    }
}

fn push_items(items: &[Item], depth: usize, repo: Option<&str>, rows: &mut Vec<Row>) {
    for item in items {
        rows.push(Row::Item {
            item: Box::new(item.clone()),
            depth,
            repo: repo.map(str::to_owned),
        });
        push_items(&item.children, depth + 1, repo, rows);
    }
}

/// One evaluated row's item + its children — `repo` tags every
/// descendant (children live in the parent's ledger).
fn push_scoped(s: Scoped, depth: usize, rows: &mut Vec<Row>) {
    let repo = s.repo;
    push_items(std::slice::from_ref(s.item), depth, repo, rows);
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
pub(crate) mod tests {
    use super::*;
    use camino::Utf8PathBuf;
    use myco_core::Section;

    pub(crate) fn t(n: u32) -> Item {
        Item::new(ItemId::new(ItemKind::Task, n), format!("t{n}"))
    }

    pub(crate) fn key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    /// The model's read store — the repo-mode fixtures always
    /// have one.
    pub(crate) fn store(m: &Model) -> &MdStore {
        m.store.as_ref().unwrap()
    }

    /// A model over a temp `project/` dir — `no_commit` keeps git
    /// out of the loop (autocommit short-circuits). The populated
    /// ledger is saved to disk too: mutations re-read under the
    /// lock, so in-memory-only items would look deleted to them.
    pub(crate) fn model(populate: impl FnOnce(&mut Ledger)) -> Model {
        let dir = tempfile::tempdir().unwrap();
        let worktree = dir.path().join("project");
        std::fs::create_dir_all(&worktree).unwrap();
        let store = MdStore::new(Utf8PathBuf::from_path_buf(worktree.clone()).unwrap());
        let mut ledger = items::load_ledger(&store).unwrap();
        populate(&mut ledger);
        items::save_ledger(&store, &ledger).unwrap();
        let mut m = Model {
            proj: Some(discover::Project {
                root: dir.path().to_path_buf(),
                worktree,
            }),
            store: Some(store),
            ledger,
            orgs: None,
            org_loader: None,
            no_commit: true,
            board: Board::Todo,
            rows: Vec::new(),
            sel: 0,
            mode: Mode::Normal,
            input: String::new(),
            filter: String::new(),
            view: None,
            cmdline: String::new(),
            show_ids: true,
            status: String::new(),
            pending_editor: None,
            files: Vec::new(),
            handoffs: Vec::new(),
            links: Vec::new(),
            watch_fp: 0,
            preview_scroll: 0,
            pending: None,
            board_area: Rect::default(),
            viewport_top: 0,
            tabs_area: None,
        };
        m.rebuild_rows();
        m.watch_fp = m.fingerprint();
        // keep the tempdir alive for the test's duration
        std::mem::forget(dir);
        m
    }

    #[test]
    fn rows_flatten_sections_and_children() {
        let m = model(|l| {
            l.todo.sections.push(Section {
                name: Some("Now".into()),
                items: vec![t(1), t(2).child(t(4))],
            });
        });
        assert!(matches!(m.rows[0], Row::Header(_)));
        assert!(matches!(m.rows[1], Row::Item { ref item, depth: 0, .. } if item.id.number() == 1));
        assert!(matches!(m.rows[3], Row::Item { ref item, depth: 1, .. } if item.id.number() == 4));
        // cursor lands on the first item, not the header
        assert_eq!(m.sel, 1);
        assert_eq!(m.selected().unwrap().id.number(), 1);
    }

    #[test]
    fn section_jumps() {
        let mut m = model(|l| {
            l.todo.sections.push(Section {
                name: Some("A".into()),
                items: vec![t(1), t(2)],
            });
            l.todo.sections.push(Section {
                name: Some("B".into()),
                items: vec![t(3)],
            });
        });
        // rows: [H(A), t1, t2, H(B), t3] — sel starts on t1
        assert_eq!(m.sel, 1);
        // } → next group's first item
        m.key(key('}'));
        assert_eq!(m.selected().unwrap().id.number(), 3);
        // } at the last group — stays
        m.key(key('}'));
        assert_eq!(m.selected().unwrap().id.number(), 3);
        // { at group start → previous group's first item
        m.key(key('{'));
        assert_eq!(m.selected().unwrap().id.number(), 1);
        // { again — already at the top group's start, stays
        m.key(key('{'));
        assert_eq!(m.selected().unwrap().id.number(), 1);
        // { mid-group → current group's first item
        m.sel = 2;
        m.key(key('{'));
        assert_eq!(m.selected().unwrap().id.number(), 1);
    }

    #[test]
    fn j_k_skip_headers_and_stop_at_edges() {
        let mut m = model(|l| {
            l.todo.sections.push(Section::named("S"));
            l.todo.sections[0].items = vec![t(1)];
            l.todo.sections.push(Section::named("T"));
            l.todo.sections[1].items = vec![t(2)];
        });
        // rows: H S, t1, H T, t2 — j from t1 lands on t2
        m.key(key('j'));
        assert_eq!(m.selected().unwrap().id.number(), 2);
        // edge: no wrap
        m.key(key('j'));
        assert_eq!(m.selected().unwrap().id.number(), 2);
        m.key(key('k'));
        assert_eq!(m.selected().unwrap().id.number(), 1);
    }

    #[test]
    fn gg_and_g_and_tab_switch() {
        let mut m = model(|l| {
            l.put_todo(t(1), None);
            l.put_todo(t(2), None);
            l.put_backlog(t(3).status(Status::Deferred), None);
        });
        m.key(key('G'));
        assert_eq!(m.selected().unwrap().id.number(), 2);
        m.key(key('g'));
        m.key(key('g'));
        assert_eq!(m.selected().unwrap().id.number(), 1);
        m.key(key('2'));
        assert_eq!(m.board, Board::Backlog);
        assert_eq!(m.selected().unwrap().id.number(), 3);
        m.key(key('1'));
        assert_eq!(m.board, Board::Todo);
    }

    #[test]
    fn dd_remembers_the_prefix_and_dones() {
        let mut m = model(|l| l.put_todo(t(1), None));
        m.key(key('d'));
        assert!(m.selected().is_some()); // still there — only armed
        m.key(key('d'));
        assert!(m.selected().is_none());
        assert!(m.ledger.locate(ItemId::new(ItemKind::Task, 1)).is_none());
    }

    #[test]
    fn defer_and_promote_cross_boards() {
        let mut m = model(|l| l.put_todo(t(1), None));
        m.key(key('D'));
        let loc = m.ledger.locate(ItemId::new(ItemKind::Task, 1)).unwrap();
        assert!(loc.backlog);
        m.key(key('2'));
        m.key(key('p'));
        let loc = m.ledger.locate(ItemId::new(ItemKind::Task, 1)).unwrap();
        assert!(!loc.backlog);
        assert_eq!(m.board, Board::Backlog); // board stays put
    }

    #[test]
    fn help_toggles_and_swallows_keys() {
        let mut m = model(|_| {});
        m.key(key('?'));
        assert_eq!(m.mode, Mode::Help);
        // other keys are swallowed, not applied to the board
        m.key(key('j'));
        assert_eq!(m.mode, Mode::Help);
        // ? closes
        m.key(key('?'));
        assert_eq!(m.mode, Mode::Normal);
        // q closes rather than quitting
        m.key(key('?'));
        assert!(!m.key(key('q')));
        assert_eq!(m.mode, Mode::Normal);
        // esc closes
        m.key(key('?'));
        m.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(m.mode, Mode::Normal);
    }

    #[test]
    fn ctrl_c_quits_from_every_mode() {
        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        for (enter, mode) in [
            (
                Box::new(|_m: &mut Model| {}) as Box<dyn Fn(&mut Model)>,
                Mode::Normal,
            ),
            (Box::new(|m: &mut Model| m.mode = Mode::Add), Mode::Add),
            (
                Box::new(|m: &mut Model| m.mode = Mode::Filter),
                Mode::Filter,
            ),
            (
                Box::new(|m: &mut Model| m.mode = Mode::Command),
                Mode::Command,
            ),
            (Box::new(|m: &mut Model| m.mode = Mode::Help), Mode::Help),
            (
                Box::new(|m: &mut Model| m.mode = Mode::Preview),
                Mode::Preview,
            ),
        ] {
            let mut m = model(|_| {});
            enter(&mut m);
            assert_eq!(m.mode, mode);
            assert!(m.key(ctrl_c), "ctrl-c should quit from {mode:?}");
        }
        // and the chord never leaks into a text buffer
        let mut m = model(|_| {});
        m.mode = Mode::Add;
        m.key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL));
        assert!(m.input.is_empty());
    }

    #[test]
    fn slash_filters_rows_live() {
        let mut m = model(|l| {
            l.put_todo(t(1), None);
            l.put_todo(t(2), None);
        });
        m.key(key('/'));
        for c in "t2".chars() {
            m.key(key(c));
        }
        assert_eq!(m.rows.len(), 1);
        assert_eq!(m.selected().unwrap().id.number(), 2);
        // esc clears back to the full board
        m.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(m.mode, Mode::Normal);
        assert_eq!(m.rows.len(), 2);
    }

    fn click(x: u16, y: u16) -> MouseEvent {
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: x,
            row: y,
            modifiers: KeyModifiers::NONE,
        }
    }

    #[test]
    fn click_selects_items_and_wheel_moves() {
        let mut m = model(|l| {
            l.todo.sections.push(Section::named("S"));
            l.todo.sections[0].items = vec![t(1), t(2)];
        });
        // rows: H S, t1, t2 — select_row only takes Item rows
        m.select_row(0);
        assert_eq!(m.selected().unwrap().id.number(), 1);
        m.select_row(2);
        assert_eq!(m.selected().unwrap().id.number(), 2);
        m.select_row(9); // out of range — sel stays put
        assert_eq!(m.selected().unwrap().id.number(), 2);

        // a click in the board area maps viewport y → row index
        m.board_area = Rect::new(0, 1, 80, 20);
        m.viewport_top = 0;
        m.mouse(click(0, 2)); // y 2 → rows[1] = t1
        assert_eq!(m.selected().unwrap().id.number(), 1);
        // wheel over the board moves the cursor
        m.mouse(MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 0,
            row: 5,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(m.selected().unwrap().id.number(), 2);
        // clicks outside the board area don't move the cursor
        m.mouse(click(0, 0));
        assert_eq!(m.selected().unwrap().id.number(), 2);

        // a click on the tab row switches boards
        m.tabs_area = Some(Rect::new(0, 0, 80, 1));
        m.mouse(click(8, 0)); // " BACKLOG " starts at col 8
        assert_eq!(m.board, Board::Backlog);
    }

    #[test]
    fn add_mode_commits_title() {
        let mut m = model(|_| {});
        m.key(key('a'));
        for c in "ship it".chars() {
            m.key(key(c));
        }
        m.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(m.mode, Mode::Normal);
        let item = m.selected().unwrap();
        assert_eq!(item.title, "ship it");
    }

    /// Drive `:` + `line` + `Enter` — returns the key() result so
    /// `:q` tests can assert the quit flag.
    pub(crate) fn cmd(m: &mut Model, line: &str) -> bool {
        m.key(key(':'));
        assert_eq!(m.mode, Mode::Command);
        for c in line.chars() {
            m.key(key(c));
        }
        m.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
    }

    #[test]
    fn colon_w_reloads_from_disk() {
        let mut m = model(|l| l.put_todo(t(1), None));
        // another writer lands a task while the board is open
        let mut other = items::load_ledger(store(&m)).unwrap();
        other.put_todo(t(2), None);
        items::save_ledger(store(&m), &other).unwrap();
        assert_eq!(m.rows.len(), 1);
        assert!(!cmd(&mut m, "w"));
        assert_eq!(m.mode, Mode::Normal);
        assert_eq!(m.rows.len(), 2);
    }

    #[test]
    fn colon_id_and_go_jump_the_cursor() {
        let mut m = model(|l| {
            l.put_todo(t(1), None);
            l.put_todo(t(2), None);
        });
        cmd(&mut m, "T-2");
        assert_eq!(m.selected().unwrap().id.number(), 2);
        cmd(&mut m, "go T-1");
        assert_eq!(m.selected().unwrap().id.number(), 1);
        // a valid id that's not a row here reports instead of moving
        cmd(&mut m, "T-9");
        assert_eq!(m.selected().unwrap().id.number(), 1);
        assert_eq!(m.status, "T-009: not on this board");
    }

    #[test]
    fn colon_set_toggles_ids() {
        let mut m = model(|l| l.put_todo(t(1), None));
        assert!(m.show_ids);
        cmd(&mut m, "set noid");
        assert!(!m.show_ids);
        cmd(&mut m, "set id");
        assert!(m.show_ids);
        cmd(&mut m, "set bogus");
        assert_eq!(m.status, ":set takes id / noid");
    }

    #[test]
    fn colon_unknown_and_esc_cancel() {
        let mut m = model(|l| l.put_todo(t(1), None));
        assert!(!cmd(&mut m, "bogus arg"));
        assert_eq!(m.mode, Mode::Normal);
        assert_eq!(m.status, "unknown command :bogus");
        // esc drops a half-typed line and re-entry starts empty
        m.key(key(':'));
        m.key(key('w'));
        m.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(m.mode, Mode::Normal);
        m.key(key(':'));
        assert_eq!(m.cmdline, "");
    }

    #[test]
    fn colon_q_quits() {
        let mut m = model(|_| {});
        assert!(cmd(&mut m, "q"));
        let mut m = model(|_| {});
        assert!(cmd(&mut m, "q!"));
    }

    /// Write a file-backed item (Issue stands in for all file
    /// kinds) into the model's worktree.
    pub(crate) fn save_issue(m: &Model, n: u32, title: &str) {
        let item = Item::new(ItemId::new(ItemKind::Issue, n), title);
        store(m)
            .save_item(&ItemFile::new(item).body("body"))
            .unwrap();
    }

    #[test]
    fn files_board_is_read_only() {
        let mut m = model(|_| {});
        save_issue(&m, 1, "the issue");
        m.load_files();
        m.key(key('3'));
        assert_eq!(m.board, Board::Files);
        assert_eq!(m.selected().unwrap().id, ItemId::new(ItemKind::Issue, 1));
        // every mutation key is a status hint, not an exit or a write
        for c in [' ', 'D', 'p'] {
            m.status.clear();
            m.key(key(c));
            assert!(m.status.contains("read-only"), "{c:?} → {:?}", m.status);
        }
        m.status.clear();
        m.key(key('d'));
        m.key(key('d'));
        assert!(m.status.contains("read-only"));
        m.status.clear();
        m.key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::ALT));
        assert!(m.status.contains("read-only"));
        // `a` keeps its own hint
        m.status.clear();
        m.key(key('a'));
        assert!(m.status.contains("add is a TODO/BACKLOG op"));
        assert_eq!(m.mode, Mode::Normal);
        // nothing was written — the issue file is untouched
        let f = store(&m).item(ItemId::new(ItemKind::Issue, 1)).unwrap();
        assert_eq!(f.meta.title, "the issue");
    }

    #[test]
    fn files_board_nav_preview_edit_still_work() {
        let mut m = model(|_| {});
        save_issue(&m, 1, "the issue");
        m.load_files();
        m.key(key('3'));
        m.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(m.mode, Mode::Preview);
        m.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(m.mode, Mode::Normal);
        // `E` opens the item's own file
        m.key(key('E'));
        let p = m.pending_editor.take().unwrap();
        assert!(p.to_string_lossy().contains("issues"), "{p:?}");
    }

    #[test]
    fn e_on_empty_files_board_is_a_hint() {
        let mut m = model(|_| {});
        m.key(key('3'));
        assert!(m.selected().is_none());
        m.key(key('E'));
        assert!(m.pending_editor.is_none());
        assert!(!m.status.is_empty());
    }

    #[test]
    fn mutation_errors_land_in_status_not_exit() {
        let mut m = model(|l| l.put_todo(t(1), None));
        // corrupt the ledger on disk — the mutation's fresh
        // re-read fails, but the board keeps running
        std::fs::write(
            m.proj.as_ref().unwrap().worktree.join("TODO.md"),
            "garbage\n",
        )
        .unwrap();
        assert!(!m.key(key(' ')));
        assert!(!m.status.is_empty());
        assert_eq!(m.mode, Mode::Normal);
        // …and it recovers once the ledger does
        let mut fixed = items::load_ledger(store(&m)).unwrap_or_else(|_| {
            Ledger::new(
                myco_core::TodoDoc::new("x"),
                myco_core::BacklogDoc::default(),
            )
        });
        fixed.put_todo(t(1), None);
        items::save_ledger(store(&m), &fixed).unwrap();
        m.key(key(' '));
        assert!(m.status.starts_with("done T-001"), "{}", m.status);
        assert!(m.selected().is_none());
    }

    #[test]
    fn mutation_sees_writes_since_launch() {
        let mut m = model(|l| l.put_todo(t(1), None));
        // a CLI/agent lands T-2 while the board is open — the
        // stale-startup-snapshot clobber would lose it
        let mut other = items::load_ledger(store(&m)).unwrap();
        other.put_todo(t(2), None);
        items::save_ledger(store(&m), &other).unwrap();
        m.key(key(' ')); // done T-1
        let disk = items::load_ledger(store(&m)).unwrap();
        assert!(disk.locate(ItemId::new(ItemKind::Task, 1)).is_none());
        assert!(disk.locate(ItemId::new(ItemKind::Task, 2)).is_some());
        // and the in-memory board reflects the fresh ledger too
        assert!(m.ledger.locate(ItemId::new(ItemKind::Task, 2)).is_some());
        assert_eq!(m.selected().unwrap().id.number(), 2);
    }

    #[test]
    fn done_lands_on_the_next_item_not_the_top() {
        let mut m = model(|l| {
            l.put_todo(t(1), None);
            l.put_todo(t(2), None);
            l.put_todo(t(3), None);
        });
        m.key(key('j'));
        assert_eq!(m.selected().unwrap().id.number(), 2);
        m.key(key(' '));
        // T-3 slid up into T-2's slot — the cursor follows it
        assert_eq!(m.selected().unwrap().id.number(), 3);
        // at the end of the board it falls back to the item before
        m.key(key(' '));
        assert_eq!(m.selected().unwrap().id.number(), 1);
    }

    #[test]
    fn reload_picks_up_new_files() {
        let mut m = model(|_| {});
        assert!(m.files.is_empty());
        save_issue(&m, 1, "landed mid-session");
        m.key(key('R'));
        assert_eq!(m.files.len(), 1);
        m.key(key('3'));
        assert_eq!(m.selected().unwrap().id, ItemId::new(ItemKind::Issue, 1));
    }

    #[test]
    fn tick_reloads_on_external_writes() {
        let mut m = model(|l| l.put_todo(t(1), None));
        // a CLI/agent lands T-2 while the board is open — the
        // poll-time fingerprint picks it up with no keypress
        let mut other = items::load_ledger(store(&m)).unwrap();
        other.put_todo(t(2), None);
        items::save_ledger(store(&m), &other).unwrap();
        m.tick();
        assert!(m.ledger.locate(ItemId::new(ItemKind::Task, 2)).is_some());
        assert_eq!(
            m.rows
                .iter()
                .filter(|r| matches!(r, Row::Item { .. }))
                .count(),
            2
        );
        // the board's own mutation refreshes the fingerprint — a
        // following tick doesn't reload. Probe: an in-memory-only
        // item survives tick (a reload would drop it).
        m.key(key(' '));
        m.ledger.put_todo(t(9), None);
        m.tick();
        assert!(m.ledger.locate(ItemId::new(ItemKind::Task, 9)).is_some());
    }

    #[test]
    fn related_groups_edges_in_and_out() {
        let mut m = model(|l| {
            l.put_todo(t(1).blocks(ItemId::new(ItemKind::Task, 2)), None);
            l.put_todo(
                Item::new(ItemId::new(ItemKind::Task, 2), "depends on T-001"),
                None,
            );
        });
        // T-1 declares blocks; T-2's title mentions T-001 → for
        // T-1 that's an incoming `references`, read `referenced_by`
        let r1 = m.related(ItemId::new(ItemKind::Task, 1));
        assert_eq!(
            r1,
            [
                (EdgeKind::Blocks, ItemId::new(ItemKind::Task, 2)),
                (EdgeKind::ReferencedBy, ItemId::new(ItemKind::Task, 2)),
            ]
        );
        assert_eq!(
            m.related(ItemId::new(ItemKind::Task, 2)),
            [
                (EdgeKind::BlockedBy, ItemId::new(ItemKind::Task, 1)),
                (EdgeKind::References, ItemId::new(ItemKind::Task, 1)),
            ]
        );
        // file items contribute too — an issue body mention
        let issue = Item::new(ItemId::new(ItemKind::Issue, 1), "the issue");
        store(&m)
            .save_item(&ItemFile::new(issue).body("see T-001"))
            .unwrap();
        m.load_files();
        m.rebuild_rows();
        assert!(m
            .related(ItemId::new(ItemKind::Task, 1))
            .contains(&(EdgeKind::ReferencedBy, ItemId::new(ItemKind::Issue, 1))));
        // and a handoff file is a self-edge
        store(&m)
            .append_handoff(
                ItemId::new(ItemKind::Task, 1),
                &myco_core::HandoffEntry {
                    date: myco_core::date::parse("2026-01-01").unwrap(),
                    actor: "devin".parse().unwrap(),
                    session: None,
                    note: "next: T-002".into(),
                },
            )
            .unwrap();
        m.load_files();
        m.rebuild_rows();
        let r = m.related(ItemId::new(ItemKind::Task, 1));
        assert!(r.contains(&(EdgeKind::Handoff, ItemId::new(ItemKind::Task, 1))));
        // titles resolve for loaded neighbors
        assert_eq!(
            m.item_title(ItemId::new(ItemKind::Issue, 1)),
            Some("the issue")
        );
        assert_eq!(m.item_title(ItemId::new(ItemKind::Task, 1)), Some("t1"));
        assert!(m.item_title(ItemId::new(ItemKind::Task, 99)).is_none());
    }

    #[test]
    fn preview_scrolls_and_closes() {
        let mut m = model(|l| l.put_todo(t(1), None));
        m.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(m.mode, Mode::Preview);
        m.key(key('j'));
        m.key(key('j'));
        assert_eq!(m.preview_scroll, 2);
        m.key(key('k'));
        assert_eq!(m.preview_scroll, 1);
        m.key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE));
        assert_eq!(m.preview_scroll, 11);
        m.key(key('g'));
        assert_eq!(m.preview_scroll, 0);
        m.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(m.mode, Mode::Normal);
        // re-entering resets the scroll
        m.preview_scroll = 7;
        m.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(m.mode, Mode::Preview);
        assert_eq!(m.preview_scroll, 0);
    }

    // ---- index-mode (`source = "index"` views) ------------------

    use myco_core::{BacklogDoc, GroupAxis, TodoDoc};

    /// A repo slice with a fake worktree — for boards that never
    /// mutate, the path is inert.
    pub(crate) fn org(name: &str, items: Vec<Item>) -> OrgRepo {
        OrgRepo {
            name: name.into(),
            worktree: PathBuf::from(format!("/nope/{name}/project")),
            items,
            archived: BTreeSet::new(),
            error: None,
        }
    }

    pub(crate) type OrgCell = std::rc::Rc<std::cell::RefCell<Vec<OrgRepo>>>;

    /// An index-mode model — the returned cell feeds the loader
    /// closure, so tests can swap the universe under `reload`.
    pub(crate) fn index_model(group_by: Option<GroupAxis>, orgs: Vec<OrgRepo>) -> (Model, OrgCell) {
        let cell = std::rc::Rc::new(std::cell::RefCell::new(orgs));
        let src = cell.clone();
        let mut m = Model {
            proj: None,
            store: None,
            ledger: Ledger::new(TodoDoc::new(""), BacklogDoc::default()),
            orgs: Some(cell.borrow().clone()),
            org_loader: Some(Box::new(move || Ok(src.borrow().clone()))),
            no_commit: true,
            board: Board::Todo,
            rows: Vec::new(),
            sel: 0,
            mode: Mode::Normal,
            input: String::new(),
            filter: String::new(),
            view: Some(ViewSpec {
                source: ViewSource::Index,
                group_by,
                ..ViewSpec::default()
            }),
            cmdline: String::new(),
            show_ids: true,
            status: String::new(),
            pending_editor: None,
            files: Vec::new(),
            handoffs: Vec::new(),
            links: Vec::new(),
            watch_fp: 0,
            preview_scroll: 0,
            pending: None,
            board_area: Rect::default(),
            viewport_top: 0,
            tabs_area: None,
        };
        m.rebuild_rows();
        m.watch_fp = m.fingerprint();
        (m, cell)
    }

    /// `(id, repo)` of every item row, in display order.
    fn row_tags(m: &Model) -> Vec<(String, Option<String>)> {
        m.rows
            .iter()
            .filter_map(|r| match r {
                Row::Item { item, repo, .. } => Some((item.id.to_string(), repo.clone())),
                Row::Header(_) => None,
            })
            .collect()
    }

    fn tag(id: &str, repo: &str) -> (String, Option<String>) {
        (id.to_string(), Some(repo.to_string()))
    }

    /// Header lines, in display order.
    fn headers(m: &Model) -> Vec<String> {
        m.rows
            .iter()
            .filter_map(|r| match r {
                Row::Header(h) => Some(h.clone()),
                _ => None,
            })
            .collect()
    }

    /// A real `project/` dir seeded via the store — the owning
    /// repo mutations write through to. Caller `forget`s it.
    pub(crate) fn repo_worktree(
        name: &str,
        populate: impl FnOnce(&mut Ledger),
    ) -> (tempfile::TempDir, OrgRepo) {
        let dir = tempfile::tempdir().unwrap();
        let wt = dir.path().join("project");
        std::fs::create_dir_all(&wt).unwrap();
        let store = MdStore::new(Utf8PathBuf::from_path_buf(wt.clone()).unwrap());
        let mut ledger = items::load_ledger(&store).unwrap();
        populate(&mut ledger);
        items::save_ledger(&store, &ledger).unwrap();
        (
            dir,
            OrgRepo {
                name: name.into(),
                worktree: wt,
                items: ledger
                    .todo
                    .items()
                    .chain(ledger.backlog.items())
                    .cloned()
                    .collect(),
                archived: BTreeSet::new(),
                error: None,
            },
        )
    }

    #[test]
    fn index_rows_carry_repo_and_partition_tabs() {
        let (mut m, _c) = index_model(
            None,
            vec![
                org("a", vec![t(1)]),
                org(
                    "b",
                    vec![
                        t(7),
                        t(3).status(Status::Deferred),
                        Item::new(ItemId::new(ItemKind::Issue, 2), "bug"),
                    ],
                ),
            ],
        );
        // TODO tab: live tasks only, `pri,id` order, repo-tagged
        assert_eq!(row_tags(&m), vec![tag("T-001", "a"), tag("T-007", "b")]);
        assert_eq!(m.selected_repo(), Some("a"));
        // BACKLOG: deferred tasks
        m.key(key('2'));
        assert_eq!(row_tags(&m), vec![tag("T-003", "b")]);
        // FILES: non-task kinds get kind headers like repo mode
        m.key(key('3'));
        assert_eq!(headers(&m), vec!["issues"]);
        assert_eq!(row_tags(&m), vec![tag("I-002", "b")]);
    }

    #[test]
    fn index_group_by_axes_make_headers() {
        // org-status's axis — one `##` per repo
        let (m, _c) = index_model(
            Some(GroupAxis::Repo),
            vec![org("b", vec![t(2)]), org("a", vec![t(1)])],
        );
        assert_eq!(headers(&m), vec!["a", "b"]);
        // org-board's axis — status buckets; empty lanes drop out
        let (m, _c) = index_model(
            Some(GroupAxis::Status),
            vec![org("a", vec![t(1), t(2).status(Status::InProgress)])],
        );
        assert_eq!(headers(&m), vec!["open", "in-progress"]);
    }

    #[test]
    fn index_mutation_writes_the_owning_repo() {
        let (dir, a) = repo_worktree("a", |l| l.put_todo(t(1), None));
        let wt = a.worktree.clone();
        std::mem::forget(dir); // the worktree must outlive the test
        let (mut m, _c) = index_model(None, vec![a, org("b", vec![t(7)])]);
        // `dd` on a's T-001 → the task leaves a's ledger on disk;
        // b's fake path is never touched
        m.key(key(' '));
        assert!(m.status.starts_with("done T-001"), "{}", m.status);
        assert_eq!(row_tags(&m), vec![tag("T-007", "b")]);
        let store = MdStore::new(Utf8PathBuf::from_path_buf(wt).unwrap());
        let disk = items::load_ledger(&store).unwrap();
        assert!(disk.locate(ItemId::new(ItemKind::Task, 1)).is_none());
    }

    #[test]
    fn index_mutation_hits_disk_and_splices_the_slice() {
        let (dir, a) = repo_worktree("a", |l| {
            l.put_todo(t(1), None);
            l.put_todo(t(2), None);
        });
        let wt = a.worktree.clone();
        std::mem::forget(dir);
        let (mut m, _c) = index_model(None, vec![a]);
        // defer T-1 → a's BACKLOG.md on disk
        m.key(key('D'));
        assert!(m.status.contains("deferred T-001"), "{}", m.status);
        let store = MdStore::new(Utf8PathBuf::from_path_buf(wt).unwrap());
        let disk = items::load_ledger(&store).unwrap();
        assert!(disk.locate(ItemId::new(ItemKind::Task, 1)).unwrap().backlog);
        // the slice spliced: T-1 lives on the BACKLOG tab now
        assert_eq!(row_tags(&m), vec![tag("T-002", "a")]);
        m.key(key('2'));
        assert_eq!(row_tags(&m), vec![tag("T-001", "a")]);
    }

    #[test]
    fn index_reload_re_runs_the_loader() {
        let (mut m, cell) = index_model(None, vec![org("a", vec![t(1)])]);
        assert_eq!(m.rows.len(), 1);
        cell.borrow_mut()[0].items.push(t(2));
        assert!(!cmd(&mut m, "w"));
        assert_eq!(row_tags(&m), vec![tag("T-001", "a"), tag("T-002", "a")]);
    }

    #[test]
    fn index_add_is_refused() {
        let (mut m, _c) = index_model(None, vec![org("a", vec![t(1)])]);
        m.key(key('a'));
        assert_eq!(m.mode, Mode::Normal);
        assert!(m.status.contains("inside it"), "{}", m.status);
    }

    #[test]
    fn index_filter_matches_repo_names() {
        let (mut m, _c) = index_model(
            None,
            vec![org("alpha", vec![t(1)]), org("beta", vec![t(2)])],
        );
        m.key(key('/'));
        for c in "bet".chars() {
            m.key(key(c));
        }
        assert_eq!(row_tags(&m), vec![tag("T-002", "beta")]);
    }

    #[test]
    fn index_repo_errors_surface_as_headers() {
        let mut o = org("a", vec![]);
        o.error = Some("rescan failed".into());
        let (m, _c) = index_model(None, vec![o]);
        assert_eq!(headers(&m), vec!["a: rescan failed"]);
    }

    #[test]
    fn index_files_board_reads_body_lazily() {
        let (dir, mut a) = repo_worktree("a", |_| {});
        // seed an issue file into a's real worktree
        let store = MdStore::new(Utf8PathBuf::from_path_buf(a.worktree.clone()).unwrap());
        let issue =
            ItemFile::new(Item::new(ItemId::new(ItemKind::Issue, 5), "the issue")).body("the body");
        store.save_item(&issue).unwrap();
        a.items.push(issue.meta.clone());
        std::mem::forget(dir);
        let (mut m, _c) = index_model(None, vec![a]);
        m.key(key('3'));
        assert_eq!(m.selected().unwrap().id, ItemId::new(ItemKind::Issue, 5));
        assert_eq!(
            m.file_body(ItemId::new(ItemKind::Issue, 5)).as_deref(),
            Some("the body")
        );
    }

    // ---- open / session / fingerprint / edge-path coverage ----

    /// Serialize tests that mutate `MYCO_HOME`/`MYCO_CONFIG` —
    /// `cargo test` runs tests on threads in one process.
    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        LOCK.lock().unwrap()
    }

    /// A throwaway `MYCO_HOME` + an empty `myco.toml` so `open` and
    /// session/fingerprint code never see the real user config or
    /// index. Returns (home, cfgdir) — keep both alive.
    fn env_dirs() -> (tempfile::TempDir, tempfile::TempDir) {
        let home = tempfile::tempdir().unwrap();
        let cfg = tempfile::tempdir().unwrap();
        std::fs::write(cfg.path().join("myco.toml"), "").unwrap();
        (home, cfg)
    }

    fn pin_env(home: &std::path::Path, cfg: &std::path::Path) {
        std::env::set_var("MYCO_HOME", home);
        std::env::set_var("MYCO_CONFIG", cfg.join("myco.toml"));
    }

    /// A qualifying `project/` worktree (plain dir + TODO.md) with
    /// one task — enough for `Model::open` reads.
    fn open_fixture() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let wt = dir.path().join("project");
        std::fs::create_dir_all(&wt).unwrap();
        let store = MdStore::new(Utf8PathBuf::from_path_buf(wt.clone()).unwrap());
        let mut ledger = items::load_ledger(&store).unwrap();
        ledger.put_todo(t(1), None);
        items::save_ledger(&store, &ledger).unwrap();
        (dir, wt)
    }

    #[test]
    fn open_loads_a_repo_board() {
        let _g = env_lock();
        let (home, cfg) = env_dirs();
        pin_env(home.path(), cfg.path());
        let (_dir, wt) = open_fixture();
        let m = Model::open(Some(&wt), None, true, None, None).unwrap();
        assert_eq!(m.selected().unwrap().id.number(), 1);
        assert!(m.store.is_some() && m.orgs.is_none());
        assert_eq!(m.board, Board::Todo);
    }

    #[test]
    fn open_index_view_loads_the_universe() {
        let _g = env_lock();
        let (home, cfg) = env_dirs();
        pin_env(home.path(), cfg.path());
        let (_dir, wt) = open_fixture();
        let src = std::rc::Rc::new(std::cell::RefCell::new(vec![org("a", vec![t(1)])]));
        let s2 = src.clone();
        let loader: OrgLoader = Box::new(move || Ok(s2.borrow().clone()));
        let m = Model::open(
            Some(&wt),
            None,
            true,
            Some("org-board".into()),
            Some(loader),
        )
        .unwrap();
        assert!(m.orgs.is_some() && m.store.is_none());
        assert_eq!(m.status, "view: org-board");
        // repo tag on the row proves rebuild_org_rows ran
        assert!(m
            .rows
            .iter()
            .any(|r| matches!(r, Row::Item { repo: Some(_), .. })));
    }

    #[test]
    fn open_index_view_without_a_project_still_works() {
        let _g = env_lock();
        let (home, cfg) = env_dirs();
        pin_env(home.path(), cfg.path());
        // a file, not a dir — discovery fails, index mode proceeds
        let f = tempfile::NamedTempFile::new().unwrap();
        let src = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let s2 = src.clone();
        let loader: OrgLoader = Box::new(move || Ok(s2.borrow().clone()));
        let m = Model::open(
            Some(f.path()),
            None,
            true,
            Some("org-board".into()),
            Some(loader),
        )
        .unwrap();
        assert!(m.proj.is_none());
        assert_eq!(
            m.status,
            "index: no repos registered — `myco index add <path>`"
        );
    }

    #[test]
    fn open_index_view_errors() {
        let _g = env_lock();
        let (home, cfg) = env_dirs();
        pin_env(home.path(), cfg.path());
        let (_dir, wt) = open_fixture();
        // no loader at all
        let err = Model::open(Some(&wt), None, true, Some("org-board".into()), None)
            .err()
            .unwrap();
        assert!(err.contains("no index"), "{err}");
        // loader failure propagates
        let err = Model::open(
            Some(&wt),
            None,
            true,
            Some("org-board".into()),
            Some(Box::new(|| Err("index down".into()))),
        )
        .err()
        .unwrap();
        assert_eq!(err, "index down");
        // unknown view name
        let err = Model::open(Some(&wt), None, true, Some("nope".into()), None)
            .err()
            .unwrap();
        assert!(!err.is_empty());
    }

    #[test]
    fn open_reports_index_repo_errors() {
        let _g = env_lock();
        let (home, cfg) = env_dirs();
        pin_env(home.path(), cfg.path());
        let (_dir, wt) = open_fixture();
        let src = std::rc::Rc::new(std::cell::RefCell::new(vec![{
            let mut o = org("a", vec![]);
            o.error = Some("rescan failed".into());
            o
        }]));
        let s2 = src.clone();
        let loader: OrgLoader = Box::new(move || Ok(s2.borrow().clone()));
        let m = Model::open(
            Some(&wt),
            None,
            true,
            Some("org-board".into()),
            Some(loader),
        )
        .unwrap();
        assert_eq!(m.status, "index: a: rescan failed");
    }

    #[test]
    fn open_named_repo_view_filters_rows() {
        let _g = env_lock();
        let (home, cfg) = env_dirs();
        pin_env(home.path(), cfg.path());
        let (_dir, wt) = open_fixture();
        let m = Model::open(Some(&wt), None, true, Some("board".into()), None).unwrap();
        assert_eq!(m.status, "view: board");
        assert!(m.view.is_some());
    }

    #[test]
    fn load_files_reports_unreadable_kinds() {
        let mut m = model(|_| {});
        let wt = m.proj.as_ref().unwrap().worktree.clone();
        // `issues` as a file — list_items can't read it as a dir
        std::fs::write(wt.join("issues"), "not a dir").unwrap();
        m.load_files();
        assert!(m.status.contains("issues"), "{}", m.status);
        // handoffs dir with a non-UTF8 file — read_handoffs fails
        let hd = wt.join("handoffs");
        std::fs::create_dir_all(&hd).unwrap();
        std::fs::write(hd.join("T-001.md"), [0xFF, 0xFE]).unwrap();
        m.load_files();
        assert!(m.status.contains("handoffs"), "{}", m.status);
    }

    #[test]
    fn file_body_reads_the_loaded_file() {
        let mut m = model(|_| {});
        save_issue(&m, 1, "the issue");
        m.load_files();
        assert_eq!(
            m.file_body(ItemId::new(ItemKind::Issue, 1)).as_deref(),
            Some("body")
        );
        assert!(m.file_body(ItemId::new(ItemKind::Issue, 9)).is_none());
    }

    #[test]
    fn reload_reports_a_corrupt_ledger() {
        let mut m = model(|l| l.put_todo(t(1), None));
        let wt = m.proj.as_ref().unwrap().worktree.clone();
        // TODO.md as a directory — the ledger read fails
        std::fs::remove_file(wt.join("TODO.md")).unwrap();
        std::fs::create_dir_all(wt.join("TODO.md")).unwrap();
        m.reload();
        assert!(m.status.starts_with("reload:"), "{}", m.status);
    }

    #[test]
    fn index_reload_reports_loader_errors() {
        let (mut m, _c) = index_model(None, vec![org("a", vec![t(1)])]);
        m.org_loader = Some(Box::new(|| Err("index down".into())));
        m.reload();
        assert_eq!(m.status, "reload: index down");
    }

    #[test]
    fn nearest_item_row_edges() {
        let mut m = model(|l| {
            l.todo.sections.push(Section {
                name: Some("A".into()),
                items: vec![t(1)],
            });
            l.todo.sections.push(Section::named("Trailing"));
        });
        m.rebuild_rows();
        let n = m.rows.len();
        assert!(matches!(m.rows[n - 1], Row::Header(_)));
        // trailing header → falls back to the last item before it
        assert_eq!(m.nearest_item_row(n - 1), Some(1));
        // out-of-range clamps to the trailing header
        assert_eq!(m.nearest_item_row(n + 10), Some(1));
        // leading header → next item row
        assert_eq!(m.nearest_item_row(0), Some(1));
        // empty board → None
        let e = model(|_| {});
        assert_eq!(e.nearest_item_row(0), None);
    }

    #[test]
    fn jump_section_before_any_header_lands_on_first_item() {
        let mut m = model(|l| {
            l.todo.sections.push(Section::named("S"));
            l.put_todo(t(1), None);
        });
        m.rebuild_rows();
        m.sel = 0; // the header row itself
        m.key(key('{'));
        assert_eq!(m.selected().unwrap().id.number(), 1);
    }

    #[test]
    fn board_path_targets_each_board() {
        let mut m = model(|l| {
            l.put_todo(t(1), None);
            l.put_backlog(t(2).status(Status::Deferred), None);
        });
        m.key(key('E'));
        assert!(m.pending_editor.as_deref().unwrap().ends_with("TODO.md"));
        m.key(key('2'));
        m.key(key('E'));
        assert!(m.pending_editor.as_deref().unwrap().ends_with("BACKLOG.md"));
        // Files board, nothing selected → status hint, no editor
        m.key(key('3'));
        m.pending_editor = None;
        m.key(key('E'));
        assert_eq!(m.pending_editor, None);
        assert!(!m.status.is_empty());
        // with an issue loaded, E opens its file
        save_issue(&m, 1, "the issue");
        m.load_files();
        m.key(key('3'));
        m.key(key('j')); // past the "issues" header onto the item
        m.key(key('E'));
        let p = m.pending_editor.as_deref().unwrap().to_path_buf();
        assert!(p.to_string_lossy().contains("issues/"));
    }

    #[test]
    fn index_board_path_uses_the_owning_repo() {
        let (dir, a) = repo_worktree("a", |l| {
            l.put_todo(t(1), None);
            l.put_backlog(t(2).status(Status::Deferred), None);
        });
        let wt = a.worktree.clone();
        std::mem::forget(dir);
        let (mut m, _c) = index_model(None, vec![a]);
        m.key(key('E'));
        assert_eq!(m.pending_editor, Some(wt.join("TODO.md")));
        // Backlog board — T-002 deferred lives there
        m.key(key('2'));
        m.key(key('E'));
        assert_eq!(m.pending_editor, Some(wt.join("BACKLOG.md")));
        // An empty board → "nothing to edit"
        m.pending_editor = None;
        m.key(key('3'));
        m.key(key('E'));
        assert_eq!(m.pending_editor, None);
        assert!(!m.status.is_empty());
    }

    #[test]
    fn normal_mode_keys_h_l_arrows_and_wildcards() {
        let mut m = model(|l| {
            l.put_todo(t(1), None);
            l.put_todo(t(2), None);
        });
        // h/l + arrow keys cycle boards
        m.key(key('l'));
        assert_eq!(m.board, Board::Backlog);
        m.key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        assert_eq!(m.board, Board::Todo);
        m.key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
        assert_eq!(m.board, Board::Backlog);
        m.key(key('h'));
        assert_eq!(m.board, Board::Todo);
        // unbound key is a no-op
        m.key(KeyEvent::new(KeyCode::F(5), KeyModifiers::NONE));
        assert_eq!(m.board, Board::Todo);
        // F clears an active filter
        m.filter = "zzz".into();
        m.rebuild_rows();
        assert!(m.rows.is_empty());
        m.key(key('F'));
        assert_eq!(m.filter, "");
        assert!(!m.rows.is_empty());
        // q quits
        assert!(m.key(key('q')));
    }

    #[test]
    fn alt_arrows_reorder_and_alt_j_k_too() {
        let mut m = model(|l| {
            l.put_todo(t(1), None);
            l.put_todo(t(2), None);
            l.todo.sections.push(Section::named("S")); // a header row too
        });
        let order = |m: &Model| {
            m.rows
                .iter()
                .filter_map(|r| match r {
                    Row::Item { item, .. } => Some(item.id.number()),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        let alt = |c: char| KeyEvent::new(KeyCode::Char(c), KeyModifiers::ALT);
        // alt+j swaps T-001 below T-002; cursor follows the item
        m.key(alt('j'));
        assert_eq!(order(&m), vec![2, 1]);
        assert_eq!(m.selected().unwrap().id.number(), 1);
        // alt+Up swaps back (arrow-key variant of alt+k)
        m.key(KeyEvent::new(KeyCode::Up, KeyModifiers::ALT));
        assert_eq!(order(&m), vec![1, 2]);
        // edge: alt+k on the first item is a no-op
        m.key(alt('k'));
        assert_eq!(order(&m), vec![1, 2]);
    }

    #[test]
    fn reorder_reports_a_stale_selection() {
        let mut m = model(|l| l.put_todo(t(1), None));
        // external writer deletes T-001
        let mut other = items::load_ledger(store(&m)).unwrap();
        let loc = other.locate(ItemId::new(ItemKind::Task, 1)).unwrap();
        other.take(&loc);
        items::save_ledger(store(&m), &other).unwrap();
        m.key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::ALT));
        assert!(m.status.contains("no such task"), "{}", m.status);
    }

    #[test]
    fn reorder_at_the_boundary_is_a_noop() {
        // a lone item can't move — `move_within` reports false and
        // the mutation returns before save/commit
        let mut m = model(|l| l.put_todo(t(1), None));
        m.key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::ALT));
        m.key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::ALT));
        assert!(m.status.is_empty(), "{}", m.status);
        assert_eq!(m.selected().unwrap().id.number(), 1);
    }

    #[test]
    fn mutations_noop_on_an_empty_board() {
        let mut m = model(|_| {});
        let alt_j = KeyEvent::new(KeyCode::Char('j'), KeyModifiers::ALT);
        for ev in [key(' '), key('D'), key('p'), alt_j] {
            m.key(ev);
        }
        assert!(m.rows.is_empty());
    }

    #[test]
    fn mutation_status_hints_for_wrong_states() {
        let mut m = model(|l| {
            l.put_todo(t(1), None);
            l.put_backlog(t(2).status(Status::Deferred), None);
        });
        // promote on a TODO item — "isn't in BACKLOG.md"
        m.key(key('p'));
        assert!(m.status.contains("BACKLOG"), "{}", m.status);
        // BACKLOG board: space on a deferred item, D on a deferred item
        m.key(key('2'));
        m.key(key(' '));
        assert!(m.status.contains("deferred"), "{}", m.status);
        m.key(key('D'));
        assert!(m.status.contains("already deferred"), "{}", m.status);
        // external delete → done reports "no such task"
        let mut other = items::load_ledger(store(&m)).unwrap();
        let loc = other.locate(ItemId::new(ItemKind::Task, 2)).unwrap();
        other.take(&loc);
        items::save_ledger(store(&m), &other).unwrap();
        m.key(key(' '));
        assert!(m.status.contains("no such task"), "{}", m.status);
        // defer's locate arm — same trick: T-001 gone from disk but
        // still on the in-memory board
        let mut other = items::load_ledger(store(&m)).unwrap();
        if let Some(loc) = other.locate(ItemId::new(ItemKind::Task, 1)) {
            other.take(&loc);
        }
        items::save_ledger(store(&m), &other).unwrap();
        m.key(key('1')); // TODO board — the stale row is still selected
        m.key(key('D'));
        assert!(m.status.contains("no such task"), "{}", m.status);
    }

    #[test]
    fn promote_on_a_deleted_item_reports_no_such_task() {
        let mut m = model(|l| {
            l.put_backlog(t(2).status(Status::Deferred), None);
        });
        // external delete — the board still shows the stale row
        let mut other = items::load_ledger(store(&m)).unwrap();
        let loc = other.locate(ItemId::new(ItemKind::Task, 2)).unwrap();
        other.take(&loc);
        items::save_ledger(store(&m), &other).unwrap();
        m.key(key('2')); // BACKLOG board
        m.key(key('p'));
        assert!(m.status.contains("no such task"), "{}", m.status);
    }

    #[test]
    fn index_mutation_on_a_missing_repo_errors_in_status() {
        let (mut m, _c) = index_model(None, vec![org("a", vec![t(1)]), org("b", vec![t(2)])]);
        m.key(key('j')); // T-002 in repo b — /nope worktree
        assert_eq!(m.selected_repo(), Some("b"));
        m.key(key(' '));
        assert!(m.status.contains("not a directory"), "{}", m.status);
    }

    #[test]
    fn add_on_backlog_creates_a_deferred_item() {
        let mut m = model(|_| {});
        m.key(key('2'));
        m.key(key('a'));
        m.key(key('x'));
        m.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(m
            .ledger
            .backlog
            .items()
            .any(|i| i.status == Status::Deferred));
    }

    #[test]
    fn session_roundtrip_restores_board_and_item() {
        let _g = env_lock();
        let (home, cfg) = env_dirs();
        pin_env(home.path(), cfg.path());
        let mut m = model(|l| l.put_todo(t(1), None));
        save_issue(&m, 1, "i");
        m.load_files();
        m.key(key('3')); // Files board
        assert_eq!(m.board, Board::Files);
        m.save_session();
        // drift, then restore
        m.board = Board::Todo;
        m.restore_session();
        assert_eq!(m.board, Board::Files);
        assert_eq!(m.selected().unwrap().id, ItemId::new(ItemKind::Issue, 1));
        // backlog + todo + unknown arms
        for b in [Board::Backlog, Board::Todo] {
            m.board = b;
            m.save_session();
            m.board = Board::Files;
            m.restore_session();
            assert_eq!(m.board, b);
        }
        // hand-edited session file with a bogus board → Todo
        let key = m.session_key();
        let p = session_path(&key);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, "board = \"bogus\"\nitem = \"not-an-id\"\n").unwrap();
        m.restore_session();
        assert_eq!(m.board, Board::Todo);
        // fallback key: no proj, no orgs → "index"
        m.orgs = None;
        m.proj = None;
        assert_eq!(m.session_key(), PathBuf::from("index"));
        m.save_session();
    }

    #[test]
    fn index_session_roundtrip() {
        let _g = env_lock();
        let (home, cfg) = env_dirs();
        pin_env(home.path(), cfg.path());
        let (mut m, _c) = index_model(None, vec![org("a", vec![t(1)])]);
        assert_eq!(m.session_key(), PathBuf::from("index"));
        m.key(key('2')); // BACKLOG board (empty — still saves)
        m.save_session();
        m.board = Board::Todo;
        m.restore_session();
        assert_eq!(m.board, Board::Backlog);
    }

    #[test]
    fn data_dir_prefers_myco_home_then_xdg() {
        let _g = env_lock();
        let (home, _cfg) = env_dirs();
        std::env::set_var("MYCO_HOME", home.path());
        assert_eq!(data_dir(), home.path().to_path_buf());
        std::env::remove_var("MYCO_HOME");
        let xdg = tempfile::tempdir().unwrap();
        std::env::set_var("XDG_DATA_HOME", xdg.path());
        assert_eq!(data_dir(), xdg.path().join("myco"));
        std::env::remove_var("XDG_DATA_HOME");
        let _ = data_dir(); // HOME fallback arm
    }

    #[test]
    fn fingerprint_covers_git_skip_and_unreadable_dirs() {
        let mut m = model(|l| l.put_todo(t(1), None));
        let wt = m.proj.as_ref().unwrap().worktree.clone();
        // `.git` marker file — name-filtered inside tree_fp
        std::fs::write(wt.join(".git"), "gitdir: elsewhere").unwrap();
        // unreadable subdir → read_dir error arm inside tree_fp
        let locked = wt.join("locked");
        std::fs::create_dir_all(&locked).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
        }
        let fp = m.fingerprint();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        assert_ne!(fp, m.watch_fp);
        // orgs None + proj None → the fingerprint's `None` arm
        m.proj = None;
        let _ = m.fingerprint();
    }

    #[test]
    fn preview_mode_scroll_keys() {
        let mut m = rich_item_model();
        m.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(m.mode, Mode::Preview);
        m.key(KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE));
        assert_eq!(m.preview_scroll, 0); // saturates at 0
        m.key(KeyEvent::new(KeyCode::Char('G'), KeyModifiers::NONE));
        assert_eq!(m.preview_scroll, u16::MAX);
        m.key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE));
        m.key(KeyEvent::new(KeyCode::F(1), KeyModifiers::NONE)); // wildcard
        assert_eq!(m.mode, Mode::Preview);
        m.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(m.mode, Mode::Normal);
    }

    /// A model with one richly-fielded task for preview tests.
    fn rich_item_model() -> Model {
        model(|l| {
            l.put_todo(
                t(1).priority(myco_core::Priority::High)
                    .owner("devin".parse().unwrap())
                    .note("n")
                    .label("l")
                    .epic("e")
                    .milestone("m")
                    .child(t(2))
                    .blocks(ItemId::new(ItemKind::Task, 3)),
                None,
            );
            l.put_todo(t(2), None);
            l.put_todo(t(3), None);
        })
    }

    #[test]
    fn enter_on_an_empty_board_stays_normal() {
        let mut m = model(|_| {});
        m.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(m.mode, Mode::Normal);
    }

    #[test]
    fn filter_mode_keys() {
        let mut m = model(|l| {
            l.put_todo(t(1), None);
            l.put_todo(t(2), None);
        });
        m.key(key('/'));
        assert_eq!(m.mode, Mode::Filter);
        m.key(key('1')); // filters live
        assert_eq!(m.filter, "1");
        assert!(m.rows.len() <= 2);
        m.key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
        assert_eq!(m.filter, "");
        m.key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)); // wildcard
        assert_eq!(m.mode, Mode::Filter);
        m.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(m.mode, Mode::Normal);
        // esc clears the filter outright
        m.key(key('/'));
        m.key(key('x'));
        m.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(m.filter, "");
        assert_eq!(m.mode, Mode::Normal);
    }

    #[test]
    fn command_mode_key_edges() {
        let mut m = model(|l| l.put_todo(t(1), None));
        m.key(key(':'));
        assert_eq!(m.mode, Mode::Command);
        m.key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE)); // empty pop
        m.key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)); // wildcard
        assert_eq!(m.mode, Mode::Command);
        m.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)); // "" → no-op
        assert_eq!(m.mode, Mode::Normal);
        // :e on a board with an item → pending editor
        cmd(&mut m, "e");
        assert!(m.pending_editor.is_some());
        // :go with a bad id reports, doesn't jump
        cmd(&mut m, "go bogus");
        assert!(m.status.contains("go expects"), "{}", m.status);
    }

    #[test]
    fn add_mode_key_edges() {
        let mut m = model(|_| {});
        m.key(key('a'));
        assert_eq!(m.mode, Mode::Add);
        m.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(m.mode, Mode::Normal);
        m.key(key('a'));
        m.key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)); // wildcard
        m.key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
        m.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)); // empty title
        assert_eq!(m.mode, Mode::Normal);
        assert!(m.ledger.todo.items().next().is_none());
    }

    #[test]
    fn mouse_edges() {
        let mut m = model(|l| {
            l.put_todo(t(1), None);
            l.put_todo(t(2), None);
        });
        m.board_area = Rect::new(0, 1, 80, 20);
        m.tabs_area = Some(Rect::new(0, 0, 80, 1));
        // ignored outside Normal mode
        m.mode = Mode::Add;
        m.mouse(MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 5,
            row: 5,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(m.sel, 0);
        m.mode = Mode::Normal;
        let scroll = |kind| MouseEvent {
            kind,
            column: 5,
            row: 5,
            modifiers: KeyModifiers::NONE,
        };
        // wheel inside the board moves the cursor both ways
        m.mouse(scroll(MouseEventKind::ScrollDown));
        assert_eq!(m.sel, 1);
        m.mouse(scroll(MouseEventKind::ScrollUp));
        assert_eq!(m.sel, 0);
        // wheel outside the board area is ignored
        m.mouse(MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 5,
            row: 0,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(m.sel, 0);
        // click on the tab row switches boards
        m.mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 10,
            row: 0,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(m.board, Board::Backlog);
        // click on empty tab-row space hits no tab → no switch
        m.mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 79,
            row: 0,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(m.board, Board::Backlog);
        // click inside the board selects the row's item
        // (row 0 of the board is the header — item rows start at 1)
        m.switch_board(0); // TODO, rows rebuilt
        m.mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 5,
            row: 2,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(m.selected().unwrap().id.number(), 2);
        // a non-down/non-scroll kind is ignored
        m.mouse(MouseEvent {
            kind: MouseEventKind::Moved,
            column: 5,
            row: 5,
            modifiers: KeyModifiers::NONE,
        });
    }

    #[test]
    fn jump_section_forward_and_dead_ends() {
        let mut m = model(|l| {
            l.todo.sections.push(Section {
                name: Some("A".into()),
                items: vec![t(1)],
            });
            l.todo.sections.push(Section {
                name: Some("B".into()),
                items: vec![t(2)],
            });
        });
        m.rebuild_rows();
        // '}' jumps forward to the next section's first item
        m.key(key('}'));
        assert_eq!(m.selected().unwrap().id.number(), 2);
        // a header with no items after it → the `None` arm is a no-op
        let mut m2 = model(|l| {
            l.todo.sections.push(Section {
                name: Some("A".into()),
                items: vec![t(1)],
            });
            l.todo.sections.push(Section::named("T1"));
            l.todo.sections.push(Section::named("T2"));
        });
        m2.rebuild_rows();
        m2.sel = m2.rows.len() - 1; // sit on the last (empty) header
        let before = m2.sel;
        m2.key(key('{'));
        assert_eq!(m2.sel, before);
        // '{' on an empty board is a no-op too
        let mut e = model(|_| {});
        e.key(key('{'));
        assert!(e.rows.is_empty());
    }

    #[test]
    fn reload_fallthrough_paths() {
        // index model with no loader → reload keeps rows, no panic
        let (mut m, _c) = index_model(None, vec![org("a", vec![t(1)])]);
        m.org_loader = None;
        m.reload();
        assert!(!m.rows.is_empty());
        // neither orgs nor store → reload is a no-op beyond resync
        let mut r = model(|l| l.put_todo(t(1), None));
        r.store = None;
        r.reload();
        // fingerprint with orgs None + proj None → `None` arm
        r.proj = None;
        let _ = r.fingerprint();
    }

    #[test]
    fn index_files_board_path_targets_the_item_file() {
        let (dir, mut a) = repo_worktree("a", |_| {});
        let store = MdStore::new(Utf8PathBuf::from_path_buf(a.worktree.clone()).unwrap());
        let issue =
            ItemFile::new(Item::new(ItemId::new(ItemKind::Issue, 5), "idx issue")).body("b");
        store.save_item(&issue).unwrap();
        a.items.push(issue.meta.clone());
        std::mem::forget(dir);
        let (mut m, _c) = index_model(None, vec![a]);
        m.key(key('3'));
        m.key(key('j')); // onto the issue row
        m.key(key('E'));
        let p = m.pending_editor.as_deref().unwrap();
        assert!(p.to_string_lossy().contains("issues/"), "{p:?}");
    }

    #[test]
    fn apply_ledger_ignores_an_unknown_repo() {
        let (mut m, _c) = index_model(None, vec![org("a", vec![t(1)])]);
        let ledger = items::load_ledger(&MdStore::new(
            Utf8PathBuf::from_path_buf(m.orgs.as_ref().unwrap()[0].worktree.clone()).unwrap(),
        ))
        .unwrap();
        // a repo name that isn't in the universe → the find fails,
        // rows stay as they were
        m.apply_ledger(Some("ghost"), ledger);
        assert!(m.orgs.as_ref().unwrap()[0]
            .items
            .iter()
            .any(|i| i.id.number() == 1));
    }

    #[test]
    fn load_files_handoffs_as_a_file() {
        let mut m = model(|_| {});
        let wt = m.proj.as_ref().unwrap().worktree.clone();
        // `handoffs` as a plain file — the wholesale list fails
        std::fs::write(wt.join("handoffs"), "not a dir").unwrap();
        m.load_files();
        assert!(m.status.contains("handoffs"), "{}", m.status);
    }
}
