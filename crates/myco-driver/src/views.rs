//! views — the driver's manifest: every ViewSpec plus the query
//! resolvers that feed them.
//!
//! Read path: all data comes from [`crate::data::Snapshot`] — a
//! single shared-lock read of the resolved `project/` worktree per
//! wire op, through `myco-ops`/`myco-store-md` exactly like the
//! CLI verbs. Nothing here proxies `myco serve`: the reads are
//! cheap enough in-process, and keeping the driver a pure reader
//! means it can never mutate the ledger.
//!
//! TUI → view mapping (the retirement checklist — `myco-tui`'s
//! screens and where each landed):
//!
//! | TUI surface                     | pavo view            |
//! |--------------------------------|----------------------|
//! | TODO tab (sections + children)  | `myco:board` tree     |
//! | BACKLOG tab                     | `myco:board` tree     |
//! | FILES tab (kind groups)         | `myco:items` + `myco:{issues,proposals,adrs,plans}` |
//! | `Enter` preview pane            | `myco:item` (`--param id=`) |
//! | preview "related" section       | `myco:item` related table / `myco:graph` |
//! | status bar / `myco status`      | `myco:status` kv       |
//! | `/` filter                      | `myco:items --param kind=/status=` (query-side) |
//! | `--view org-*` index boards     | NOT PORTED — needs the redb index; follow-up |
//! | mutations (`space`, `a`, `dd`…) | NOT PORTED — writes stay on `myco` verbs/`myco serve` |

use std::collections::BTreeMap;

use myco_core::{Item, ItemId};
use pavo_driver_kit as kit;
use pavo_protocol::{DriverManifest, GraphNode, NodeBase, TreeItem, ViewNode, ViewSpec};
use serde_json::{json, Map, Value};

use crate::data::Snapshot;

// ---------------------------------------------------------------- helpers

fn titled(title: &str) -> NodeBase {
    kit::base().title(title).build()
}

fn bound(title: &str, query: &str) -> NodeBase {
    kit::base().title(title).query(query).build()
}

fn col(title: &str, children: Vec<ViewNode>) -> ViewNode {
    kit::column(titled(title), children)
}

fn kv(title: &str, query: &str) -> ViewNode {
    kit::kv(bound(title, query), Vec::new())
}

fn table(title: &str, query: &str, columns: &[(&str, &str)]) -> ViewNode {
    // `sortable_table`: every table this driver declares sorts.
    kit::sortable_table(
        bound(title, query),
        columns
            .iter()
            .map(|(key, t)| kit::table_column(key, t))
            .collect(),
        Vec::new(),
    )
}

fn tree(title: &str, query: &str) -> ViewNode {
    kit::expandable_tree(bound(title, query), Vec::new())
}

fn graph(title: &str, query: &str) -> ViewNode {
    // The kit's `graph` constructor leaves `layout` unset; the board
    // asks for `dot` — keep the literal for that one hint.
    ViewNode::Graph(GraphNode {
        base: bound(title, query),
        nodes: Vec::new(),
        edges: Vec::new(),
        layout: Some("dot".to_string()),
    })
}

fn md(title: &str, text: &str) -> ViewNode {
    kit::markdown(titled(title), text)
}

fn spec(
    id: &str,
    title: &str,
    route: &str,
    description: &str,
    queries: &[&str],
    root: ViewNode,
) -> ViewSpec {
    let mut view = kit::view(id, title, route, root).description(description);
    for q in queries {
        view = view.query(kit::binding(q, "query"));
    }
    view.build()
}

// ---------------------------------------------------------------- manifest

/// The file-backed builtin kinds get one view each — the TUI's
/// FILES tab grouped by kind; `myco:items` is the combined table.
const FILE_VIEWS: [(&str, &str, &str); 4] = [
    ("issues", "Issues", "issues/"),
    ("proposals", "Proposals", "proposals/"),
    ("adrs", "ADRs", "adrs/"),
    ("plans", "Plans", "plans/"),
];

const ITEM_COLS: &[(&str, &str)] = &[
    ("kind", "Kind"),
    ("id", "ID"),
    ("title", "Title"),
    ("status", "Status"),
    ("priority", "Pri"),
    ("owner", "Owner"),
    ("labels", "Labels"),
    ("source", "Source"),
    ("epic", "Epic"),
    ("milestone", "MS"),
];

const FILE_COLS: &[(&str, &str)] = &[
    ("id", "ID"),
    ("title", "Title"),
    ("status", "Status"),
    ("priority", "Pri"),
    ("owner", "Owner"),
    ("labels", "Labels"),
    ("created", "Created"),
];

/// Every view the driver declares — static (no project state
/// needed; `manifest` must answer anywhere).
pub fn specs() -> Vec<ViewSpec> {
    let mut views = vec![
        spec(
            "status",
            "Status",
            "/myco/status",
            "Ledger digest — repo, per-status and per-kind counts (the TUI status bar / `myco status`).",
            &["summary"],
            col(
                "Myco status",
                vec![kv("Ledger", "summary")],
            ),
        ),
        spec(
            "board",
            "Board",
            "/myco/board",
            "TODO.md + BACKLOG.md as a section tree with nested items — the TUI's TODO/BACKLOG tabs.",
            &["summary", "ledger"],
            col(
                "Myco board",
                vec![
                    kv("Ledger", "summary"),
                    tree("Work items", "ledger"),
                ],
            ),
        ),
        spec(
            "items",
            "Items",
            "/myco/items",
            "Every item — ledger tasks plus file-backed kinds — as one table. Filter with --param kind= / status= / source=.",
            &["items"],
            table("Items", "items", ITEM_COLS),
        ),
        spec(
            "item",
            "Item",
            "/myco/item",
            "One item's detail — metadata, related edges, children, body. Requires --param id=T-001 (any kind prefix).",
            &["item", "related", "children", "body"],
            col(
                "Item",
                vec![
                    kv("Item", "item"),
                    table(
                        "Related",
                        "related",
                        &[("kind", "Kind"), ("other", "Item"), ("title", "Title")],
                    ),
                    table(
                        "Children",
                        "children",
                        &[
                            ("id", "ID"),
                            ("title", "Title"),
                            ("status", "Status"),
                            ("priority", "Pri"),
                            ("owner", "Owner"),
                            ("depth", "Depth"),
                        ],
                    ),
                    md("Body", ""),
                ],
            ),
        ),
        spec(
            "graph",
            "Graph",
            "/myco/graph",
            "The live link graph — blocks/supersedes/links/references edges across ledger items, file items, and handoffs. `pavo view myco:graph --format dot`.",
            &["summary", "graph"],
            col(
                "Myco graph",
                vec![kv("Ledger", "summary"), graph("Links", "graph")],
            ),
        ),
        spec(
            "handoffs",
            "Handoffs",
            "/myco/handoffs",
            "handoffs/*.md files — one row per item with entry count and the latest note.",
            &["handoffs"],
            table(
                "Handoffs",
                "handoffs",
                &[
                    ("id", "ID"),
                    ("entries", "Entries"),
                    ("archived", "Archived"),
                    ("updated", "Updated"),
                    ("actor", "Actor"),
                    ("note", "Latest note"),
                ],
            ),
        ),
    ];
    for (id, title, _dir) in FILE_VIEWS {
        let lower = title.to_lowercase();
        views.push(spec(
            id,
            title,
            &format!("/myco/{id}"),
            &format!("File-backed {lower} — the TUI FILES tab's {id}/ group."),
            &[id],
            table(title, id, FILE_COLS),
        ));
    }
    views
}

/// The `manifest` op result.
pub fn manifest() -> DriverManifest {
    // `contribution` defaults priority to 100; the slots below order
    // explicitly (lower sorts first).
    let mut navbar = kit::contribution("board", "Myco", "/myco/board");
    navbar.priority = 20;
    let mut dashboard = kit::contribution("status", "Myco status", "/myco/status");
    dashboard.priority = 30;
    let mut statusbar = kit::contribution("status", "Myco ledger", "/myco/status");
    statusbar.priority = 40;

    // `Manifest::new` already defaults verbs to ["manifest","serve"]
    // and capabilities to ["pavo"] — the same values the literal
    // declared by hand.
    let mut m = kit::Manifest::new("myco", "Myco")
        .version(env!("CARGO_PKG_VERSION"))
        .icon("📋")
        .navbar(navbar)
        .dashboard(dashboard)
        .statusbar(statusbar);
    for v in specs() {
        m = m.view(v);
    }
    m.build()
}

// ---------------------------------------------------------------- queries

/// Every query name the driver answers — the `query` op rejects
/// anything outside this set with `ok:false`.
pub fn known_query(name: &str) -> bool {
    matches!(
        name,
        "summary"
            | "ledger"
            | "items"
            | "issues"
            | "proposals"
            | "adrs"
            | "plans"
            | "item"
            | "related"
            | "children"
            | "body"
            | "graph"
            | "handoffs"
    )
}

/// `{"op":"query","query":name,"params":{…}}` → the data payload.
/// Unknown names return an `{"error": …}` object (defensive — the
/// wire layer rejects them before this runs).
pub fn query_data(name: &str, params: &Map<String, Value>, snap: &Snapshot) -> Value {
    if !snap.ok() {
        return snap.error_payload();
    }
    match name {
        "summary" => summary(snap),
        "ledger" => ledger_tree(snap),
        "items" => items_rows(snap, params),
        "issues" | "proposals" | "adrs" | "plans" => {
            let want = name.trim_end_matches('s');
            files_rows(snap, want)
        }
        "item" => item_detail(snap, params),
        "related" => related_rows(snap, params),
        "children" => children_rows(snap, params),
        "body" => body_payload(snap, params),
        "graph" => graph_payload(snap, params),
        "handoffs" => handoffs_rows(snap),
        other => json!({"error": format!("unknown query '{other}'")}),
    }
}

/// Param `id` → `ItemId`; the error string doubles as the payload.
fn param_id(params: &Map<String, Value>) -> Result<ItemId, Value> {
    let raw = params
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| json!({"error": "missing required param 'id' (e.g. --param id=T-001)"}))?;
    raw.parse::<ItemId>()
        .map_err(|e| json!({"error": format!("bad id '{raw}': {e}")}))
}

/// Optional `kind`/`status`/`source` filters on the items table.
fn item_filters(params: &Map<String, Value>) -> (Option<String>, Option<String>, Option<String>) {
    let get = |k: &str| params.get(k).and_then(Value::as_str).map(str::to_lowercase);
    (get("kind"), get("status"), get("source"))
}

fn passes_filters(
    kind_name: &str,
    status: myco_core::Status,
    source: &str,
    params: &Map<String, Value>,
) -> bool {
    let (kind, st, src) = item_filters(params);
    if let Some(k) = &kind {
        if !kind_name.to_lowercase().contains(k.as_str()) {
            return false;
        }
    }
    if let Some(s) = &st {
        if status.token() != *s {
            return false;
        }
    }
    if let Some(s) = &src {
        if !source.to_lowercase().contains(s.as_str()) {
            return false;
        }
    }
    true
}

/// Every item as a flat row object — ledger items carry
/// `source` = `todo`/`backlog` plus `section`/`depth`/`parent`;
/// file items carry `source` = the kind's dir name.
fn item_row(
    snap: &Snapshot,
    i: &Item,
    source: &str,
    section: Option<&str>,
    depth: usize,
    parent: Option<&str>,
) -> Value {
    let mut row = json!({
        "kind": snap.kinds.name_of(i.id.kind()),
        "id": i.id.to_string(),
        "title": i.title,
        "status": i.status.token(),
        "priority": i.priority.token(),
        "owner": i.owner.as_ref().map(|a| a.to_string()),
        "labels": i.labels.iter().cloned().collect::<Vec<_>>().join(" "),
        "source": source,
        "depth": depth,
    });
    if let Some(s) = section {
        row["section"] = json!(s);
    }
    if let Some(p) = parent {
        row["parent"] = json!(p);
    }
    if let Some(e) = &i.epic {
        row["epic"] = json!(e);
    }
    if let Some(m) = &i.milestone {
        row["milestone"] = json!(m);
    }
    if !i.blocks.is_empty() {
        row["blocks"] = json!(i
            .blocks
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(" "));
    }
    if !i.links.is_empty() {
        row["links"] = json!(i
            .links
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(" "));
    }
    row
}

/// Flatten an item subtree into rows (self first, then
/// descendants depth-first).
#[allow(clippy::too_many_arguments)] // row shape context — a struct would just rename the args
fn push_item_rows(
    snap: &Snapshot,
    rows: &mut Vec<Value>,
    i: &Item,
    source: &str,
    section: Option<&str>,
    depth: usize,
    parent: Option<String>,
    params: &Map<String, Value>,
) {
    let kind_name = snap.kinds.name_of(i.id.kind());
    if passes_filters(&kind_name, i.status, source, params) {
        rows.push(item_row(snap, i, source, section, depth, parent.as_deref()));
    }
    for c in &i.children {
        push_item_rows(
            snap,
            rows,
            c,
            source,
            None,
            depth + 1,
            Some(i.id.to_string()),
            params,
        );
    }
}

/// `items` query — every item in the ledger + live file items.
fn items_rows(snap: &Snapshot, params: &Map<String, Value>) -> Value {
    let mut rows = Vec::new();
    if let Some(ledger) = &snap.ledger {
        for (sections, source) in [
            (&ledger.todo.sections, "todo"),
            (&ledger.backlog.sections, "backlog"),
        ] {
            for s in sections {
                for i in &s.items {
                    push_item_rows(
                        snap,
                        &mut rows,
                        i,
                        source,
                        s.name.as_deref(),
                        0,
                        None,
                        params,
                    );
                }
            }
        }
    }
    for f in &snap.files {
        let kind_name = snap.kinds.name_of(f.meta.id.kind());
        let source = snap
            .kinds
            .spec_of(f.meta.id.kind())
            .map(|s| s.dir_name())
            .unwrap_or_else(|| kind_name.clone());
        if passes_filters(&kind_name, f.meta.status, &source, params) {
            rows.push(item_row(snap, &f.meta, &source, None, 0, None));
        }
    }
    Value::Array(rows)
}

/// `<kind>` file-view queries — one kind's live items.
fn files_rows(snap: &Snapshot, kind_name: &str) -> Value {
    let rows: Vec<Value> = snap
        .files
        .iter()
        .filter(|f| snap.kinds.name_of(f.meta.id.kind()) == kind_name)
        .map(|f| {
            let mut row = item_row(snap, &f.meta, kind_name, None, 0, None);
            if let Some(c) = &f.meta.created {
                row["created"] = json!(c.to_string());
            }
            // drop columns the file views don't declare
            if let Some(m) = row.as_object_mut() {
                for k in [
                    "source",
                    "depth",
                    "parent",
                    "epic",
                    "milestone",
                    "blocks",
                    "links",
                ] {
                    m.remove(k);
                }
            }
            row
        })
        .collect();
    Value::Array(rows)
}

/// `ledger` query — the board tree payload: `{"items":[…]}` with
/// doc → section → item → children nesting.
fn ledger_tree(snap: &Snapshot) -> Value {
    let empty = || TreeItem {
        id: None,
        label: String::new(),
        detail: None,
        icon: None,
        children: Vec::new(),
        props: Default::default(),
    };
    let mut docs = Vec::new();
    if let Some(ledger) = &snap.ledger {
        for (sections, label) in [
            (&ledger.todo.sections, "TODO.md"),
            (&ledger.backlog.sections, "BACKLOG.md"),
        ] {
            let mut doc = empty();
            doc.label = label.to_string();
            for s in sections {
                let mut section_items = Vec::new();
                for i in &s.items {
                    section_items.push(item_tree(snap, i));
                }
                match &s.name {
                    Some(name) => doc.children.push(TreeItem {
                        id: None,
                        label: name.clone(),
                        detail: None,
                        icon: None,
                        children: section_items,
                        props: Default::default(),
                    }),
                    None => doc.children.extend(section_items),
                }
            }
            docs.push(doc);
        }
    }
    json!({"items": docs})
}

/// One item as a [`TreeItem`] — label is the title, detail carries
/// the TUI's meta suffix (`status · pri:x #labels owner:y`).
fn item_tree(snap: &Snapshot, i: &Item) -> TreeItem {
    let mut detail = i.status.token().to_string();
    if i.priority.token() != "med" {
        detail.push_str(&format!(" · pri:{}", i.priority.token()));
    }
    if !i.labels.is_empty() {
        detail.push_str(&format!(
            " {}",
            i.labels
                .iter()
                .map(|l| format!("#{l}"))
                .collect::<Vec<_>>()
                .join(" ")
        ));
    }
    if let Some(o) = &i.owner {
        detail.push_str(&format!(" · owner:{}", o.as_str()));
    }
    TreeItem {
        id: Some(i.id.to_string()),
        label: i.title.clone(),
        detail: Some(detail),
        icon: None,
        children: i.children.iter().map(|c| item_tree(snap, c)).collect(),
        props: {
            let mut p = Map::new();
            p.insert("status".into(), json!(i.status.token()));
            p.insert("priority".into(), json!(i.priority.token()));
            p.insert("kind".into(), json!(snap.kinds.name_of(i.id.kind())));
            if let Some(o) = &i.owner {
                p.insert("owner".into(), json!(o.as_str()));
            }
            p
        },
    }
}

/// `summary` query — the kv object: repo identity + flat
/// per-status counts + per-kind counts + load warnings.
fn summary(snap: &Snapshot) -> Value {
    let proj = snap.project.as_ref();
    let mut by_status = BTreeMap::new();
    let mut by_kind: BTreeMap<String, usize> = BTreeMap::new();
    let mut total = 0usize;

    let mut count = |i: &Item, source_kind: String| {
        *by_status.entry(i.status.token()).or_insert(0usize) += 1;
        *by_kind.entry(source_kind).or_insert(0) += 1;
        total += 1;
    };

    if let Some(ledger) = &snap.ledger {
        for i in ledger
            .todo
            .items()
            .chain(ledger.backlog.items())
            .flat_map(Item::subtree)
        {
            count(i, "task".to_string());
        }
    }
    for f in &snap.files {
        count(&f.meta, snap.kinds.name_of(f.meta.id.kind()));
    }

    let mut out = Map::new();
    out.insert(
        "repo".into(),
        json!(proj
            .and_then(|p| p.root.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_default()),
    );
    out.insert(
        "root".into(),
        json!(proj
            .map(|p| p.root.display().to_string())
            .unwrap_or_default()),
    );
    out.insert(
        "worktree".into(),
        json!(proj
            .map(|p| p.worktree.display().to_string())
            .unwrap_or_default()),
    );
    for status in myco_core::Status::ALL {
        out.insert(
            status.token().to_string(),
            json!(by_status.get(status.token()).copied().unwrap_or(0)),
        );
    }
    out.insert("items".into(), json!(total));
    out.insert("files".into(), json!(snap.files.len()));
    out.insert("handoffs".into(), json!(snap.handoffs.len()));
    out.insert(
        "edges".into(),
        json!(snap.graph.as_ref().map_or(0, |g| g.edges.len())),
    );
    out.insert("by_kind".into(), json!(by_kind));
    out.insert("warnings".into(), json!(snap.warnings));
    Value::Object(out)
}

/// Find an item anywhere the snapshot can see — ledger subtree
/// first, then live file items, then `meta`-only fallback is not
/// needed (archive stays out of detail views).
enum Found {
    Ledger(Item),
    File(myco_core::ItemFile),
}

impl Found {
    fn item(&self) -> &Item {
        match self {
            Found::Ledger(i) => i,
            Found::File(f) => &f.meta,
        }
    }
}

fn find_item(snap: &Snapshot, id: ItemId) -> Option<Found> {
    if let Some(ledger) = &snap.ledger {
        if let Some(loc) = ledger.locate(id) {
            if let Some(i) = ledger.item(&loc) {
                return Some(Found::Ledger(i.clone()));
            }
        }
    }
    snap.files
        .iter()
        .find(|f| f.meta.id == id)
        .cloned()
        .map(Found::File)
}

/// `item` query — the detail object the kv node renders as pairs.
fn item_detail(snap: &Snapshot, params: &Map<String, Value>) -> Value {
    let id = match param_id(params) {
        Ok(id) => id,
        Err(e) => return e,
    };
    let Some(found) = find_item(snap, id) else {
        return json!({"error": format!("{id} not found in this project")});
    };
    let i = found.item();
    let source = match &found {
        Found::Ledger(_) => {
            let in_backlog = snap
                .ledger
                .as_ref()
                .and_then(|l| l.locate(id))
                .is_some_and(|l| l.backlog);
            if in_backlog { "backlog" } else { "todo" }.to_string()
        }
        Found::File(_) => snap
            .kinds
            .spec_of(id.kind())
            .map(|s| s.dir_name())
            .unwrap_or_else(|| "files".to_string()),
    };
    let mut out = json!({
        "id": i.id.to_string(),
        "kind": snap.kinds.name_of(i.id.kind()),
        "title": i.title,
        "status": i.status.token(),
        "priority": i.priority.token(),
        "labels": i.labels.iter().cloned().collect::<Vec<_>>().join(" "),
        "source": source,
        "children": i.children.len(),
    });
    if let Some(o) = &i.owner {
        out["owner"] = json!(o.as_str());
    }
    if let Some(c) = &i.created {
        out["created"] = json!(c.to_string());
    }
    if let Some(e) = &i.epic {
        out["epic"] = json!(e);
    }
    if let Some(m) = &i.milestone {
        out["milestone"] = json!(m);
    }
    if !i.blocks.is_empty() {
        out["blocks"] = json!(i
            .blocks
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(" "));
    }
    if !i.links.is_empty() {
        out["links"] = json!(i
            .links
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(" "));
    }
    if let Some(s) = &i.supersedes {
        out["supersedes"] = json!(s.to_string());
    }
    if let Some(s) = &i.superseded_by {
        out["superseded_by"] = json!(s.to_string());
    }
    if let Some(n) = &i.note {
        out["note"] = json!(n);
    }
    if let Found::File(f) = &found {
        out["body_lines"] = json!(f.body.lines().count());
    }
    out
}

/// `related` query — the TUI preview's "related" section as rows.
fn related_rows(snap: &Snapshot, params: &Map<String, Value>) -> Value {
    let id = match param_id(params) {
        Ok(id) => id,
        Err(e) => return e,
    };
    let Some(graph) = &snap.graph else {
        return json!({"error": "no graph"});
    };
    let rows: Vec<Value> = graph
        .related(id)
        .into_iter()
        .map(|r| {
            json!({
                "kind": r.kind.name(),
                "other": r.other.to_string(),
                "title": r.title.unwrap_or_default(),
            })
        })
        .collect();
    Value::Array(rows)
}

/// `children` query — an item's subtree flattened with `depth`.
fn children_rows(snap: &Snapshot, params: &Map<String, Value>) -> Value {
    let id = match param_id(params) {
        Ok(id) => id,
        Err(e) => return e,
    };
    let Some(found) = find_item(snap, id) else {
        return json!({"error": format!("{id} not found in this project")});
    };
    let mut rows = Vec::new();
    fn walk(snap: &Snapshot, rows: &mut Vec<Value>, i: &Item, depth: usize) {
        for c in &i.children {
            let pid = i.id.to_string();
            let mut row = item_row(snap, c, "", None, depth, Some(pid.as_str()));
            if let Some(m) = row.as_object_mut() {
                m.remove("source");
                m.remove("parent");
            }
            rows.push(row);
            walk(snap, rows, c, depth + 1);
        }
    }
    walk(snap, &mut rows, found.item(), 1);
    Value::Array(rows)
}

/// `body` query — the file-backed item's markdown body (task
/// items answer their `note`). String payload; the `item` view
/// inlines it into its markdown node (markdown nodes aren't
/// query-bound in the IR).
fn body_payload(snap: &Snapshot, params: &Map<String, Value>) -> Value {
    let id = match param_id(params) {
        Ok(id) => id,
        Err(e) => return e,
    };
    match find_item(snap, id) {
        Some(Found::File(f)) => Value::String(f.body.clone()),
        Some(Found::Ledger(i)) => Value::String(i.note.clone().unwrap_or_default()),
        None => json!({"error": format!("{id} not found in this project")}),
    }
}

/// `graph` query — `{"nodes":[…],"edges":[…]}` for the graph node.
/// Handoff self-edges (`src == dst`) become a node prop instead of
/// a self-loop; edge labels carry the `EdgeKind` name.
fn graph_payload(snap: &Snapshot, _params: &Map<String, Value>) -> Value {
    let Some(graph) = &snap.graph else {
        return json!({"error": "no graph"});
    };
    let mut nodes: BTreeMap<String, Value> = BTreeMap::new();
    let mut node = |id: ItemId, title: Option<&str>| {
        let key = id.to_string();
        nodes.entry(key.clone()).or_insert_with(|| {
            json!({
                "id": key,
                "label": title.unwrap_or("").to_string(),
                "kind": snap.kinds.name_of(id.kind()),
                "props": {"item_id": id.to_string()},
            })
        });
    };
    for (id, title) in &graph.titles {
        node(*id, Some(title));
    }
    let mut edges = Vec::new();
    let mut handoff_ids = Vec::new();
    for e in &graph.edges {
        if e.src == e.dst {
            if e.kind == myco_core::EdgeKind::Handoff {
                handoff_ids.push(e.src);
            }
            continue;
        }
        node(e.src, graph.titles.get(&e.src).map(String::as_str));
        node(e.dst, graph.titles.get(&e.dst).map(String::as_str));
        edges.push(json!({
            "from": e.src.to_string(),
            "to": e.dst.to_string(),
            "label": e.kind.name(),
        }));
    }
    // Untitled edge endpoints still get a node (dangling mentions).
    for e in &graph.edges {
        if e.src != e.dst {
            continue;
        }
        node(e.src, graph.titles.get(&e.src).map(String::as_str));
    }
    for id in handoff_ids {
        if let Some(n) = nodes.get_mut(&id.to_string()) {
            n["props"]["handoffs"] = json!(true);
        }
    }
    json!({
        "nodes": nodes.into_values().collect::<Vec<_>>(),
        "edges": edges,
    })
}

/// `handoffs` query — one row per handoff file.
fn handoffs_rows(snap: &Snapshot) -> Value {
    Value::Array(
        snap.handoffs
            .iter()
            .map(|h| {
                json!({
                    "id": h.id.to_string(),
                    "entries": h.entries,
                    "archived": h.archived,
                    "updated": h.updated,
                    "actor": h.actor,
                    "note": h.note,
                })
            })
            .collect(),
    )
}

// ---------------------------------------------------------------- view op

/// `{"op":"view","view":id,"params":{…}}` → [`pavo_protocol::wire::ViewResult`].
///
/// The returned spec's `root` is the manifest tree for every view
/// except `item`, whose markdown body node is inlined with the
/// resolved item's body (markdown isn't query-bound in the IR —
/// the spec is personalized per request instead).
pub fn view_result(
    view_id: &str,
    params: &Map<String, Value>,
) -> Result<pavo_protocol::wire::ViewResult, String> {
    let spec = specs()
        .into_iter()
        .find(|v| v.id == view_id)
        .ok_or_else(|| format!("unknown view: {view_id}"))?;

    let snap = crate::data::load(params);
    let mut data = Map::new();
    for b in &spec.queries {
        data.insert(b.name.clone(), query_data(&b.name, params, &snap));
    }

    let root = if view_id == "item" {
        item_root(params, &snap, &spec.root)
    } else {
        spec.root.clone()
    };

    Ok(pavo_protocol::wire::ViewResult {
        view: ViewSpec { root, ..spec },
        data,
    })
}

/// The `item` view's personalized tree — same shape as the
/// manifest spec, with the Body markdown filled in.
fn item_root(params: &Map<String, Value>, snap: &Snapshot, manifest_root: &ViewNode) -> ViewNode {
    if !snap.ok() {
        return manifest_root.clone();
    }
    let body = match body_payload(snap, params) {
        Value::String(s) => s,
        other => {
            if let Some(e) = other.get("error").and_then(Value::as_str) {
                format!("*(error: {e})*")
            } else {
                String::new()
            }
        }
    };
    // Rebuild the column: kv + related + children + body markdown.
    col(
        "Item",
        vec![
            kv("Item", "item"),
            table(
                "Related",
                "related",
                &[("kind", "Kind"), ("other", "Item"), ("title", "Title")],
            ),
            table(
                "Children",
                "children",
                &[
                    ("id", "ID"),
                    ("title", "Title"),
                    ("status", "Status"),
                    ("priority", "Pri"),
                    ("owner", "Owner"),
                    ("depth", "Depth"),
                ],
            ),
            md("Body", &body),
        ],
    )
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use serde_json::Map;

    #[test]
    fn manifest_advertises_pavo_and_lists_every_view() {
        let m = manifest();
        assert!(m.verbs.iter().any(|v| v == "serve"));
        assert!(m.capabilities.iter().any(|c| c == "pavo"));
        let ids: std::collections::BTreeSet<&str> = m.views.iter().map(|v| v.id.as_str()).collect();
        assert_eq!(ids.len(), m.views.len(), "duplicate view ids");
        for want in [
            "status", "board", "items", "item", "graph", "handoffs", "adrs",
        ] {
            assert!(ids.contains(want), "missing view {want}");
        }
    }

    #[test]
    fn every_binding_is_a_known_query() {
        for v in specs() {
            for b in &v.queries {
                assert!(known_query(&b.name), "{}: unknown query {}", v.id, b.name);
            }
        }
    }

    #[test]
    fn degraded_snapshot_yields_error_payloads() {
        let snap = Snapshot {
            error: Some("no myco project".into()),
            ..Snapshot::default()
        };
        let params = Map::new();
        for name in [
            "summary", "ledger", "items", "graph", "handoffs", "item", "body",
        ] {
            let v = query_data(name, &params, &snap);
            assert_eq!(v["error"], "no myco project", "{name}");
        }
    }

    #[test]
    fn item_query_needs_id() {
        // A *healthy-looking* snapshot still isn't reachable in a
        // unit test (discovery is IO) — exercise the param check
        // through the degraded path is meaningless, so just assert
        // the param parser's contract on its own.
        let params = Map::new();
        assert!(param_id(&params).is_err());
        let mut params = Map::new();
        params.insert("id".into(), json!("not-an-id"));
        assert!(param_id(&params).is_err());
        params.insert("id".into(), json!("T-001"));
        assert!(param_id(&params).is_ok());
    }
}
