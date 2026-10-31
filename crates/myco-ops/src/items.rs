//! Shared plumbing for the item verbs — meta/list flags, the
//! TODO↔BACKLOG ledger, state-machine walks, compact output.

use std::collections::{BTreeMap, VecDeque};
use std::path::Path;

use myco_core::{
    Actor, BacklogDoc, Item, ItemFile, ItemId, ItemKind, Priority, Section, Status, TodoDoc,
};
use myco_store::Store;
use myco_store_md::MdStore;
use serde_json::json;

use crate::discover;
use crate::ops;

/// `--pri/--owner/--label/--blocks/--epic/--milestone` — the
/// metadata flags shared by every `add`/`new` verb.
#[derive(Debug, Default, clap::Args)]
pub struct MetaFlags {
    /// Priority: low|med|high|crit.
    #[arg(long, visible_alias = "priority")]
    pub pri: Option<Priority>,
    /// Owner actor id (`devin`, `human:luis`, …).
    #[arg(long)]
    pub owner: Option<Actor>,
    /// `#label` — repeatable.
    #[arg(long)]
    pub label: Vec<String>,
    /// Ids this item blocks — comma-separated or repeatable.
    #[arg(long, value_delimiter = ',')]
    pub blocks: Vec<ItemId>,
    /// Epic name — the delivery grouping this item belongs to
    /// (`[A-Za-z0-9_.-]+`).
    #[arg(long)]
    pub epic: Option<String>,
    /// Milestone name — the target this item ships under
    /// (`[A-Za-z0-9_.-]+`).
    #[arg(long)]
    pub milestone: Option<String>,
}

impl MetaFlags {
    /// Apply the flags to a freshly-created item.
    pub fn apply(&self, item: &mut Item) -> Result<(), String> {
        if let Some(p) = self.pri {
            item.priority = p;
        }
        if let Some(o) = &self.owner {
            item.owner = Some(o.clone());
        }
        for l in &self.label {
            if !myco_core::is_valid_label(l) {
                return Err(format!("invalid label {l:?}"));
            }
            item.labels.insert(l.clone());
        }
        for b in &self.blocks {
            item.blocks.insert(*b);
        }
        for (what, name) in [("epic", &self.epic), ("milestone", &self.milestone)] {
            if let Some(name) = name {
                if !myco_core::is_valid_group_name(name) {
                    return Err(format!("invalid {what} name {name:?}"));
                }
            }
        }
        item.epic = self.epic.clone();
        item.milestone = self.milestone.clone();
        Ok(())
    }
}

/// `--status/--owner/--label` list filters, shared by every `list`.
#[derive(Debug, Default, clap::Args)]
pub struct ListFlags {
    /// Keep only items with this status.
    #[arg(long)]
    pub status: Option<Status>,
    /// Keep only items owned by this actor.
    #[arg(long)]
    pub owner: Option<String>,
    /// Keep only items carrying all of these labels (repeatable).
    #[arg(long)]
    pub label: Vec<String>,
    /// Keep only the first N items after filtering.
    #[arg(long, value_name = "N")]
    pub limit: Option<usize>,
}

impl ListFlags {
    /// Does `item` pass the filters?
    pub fn matches(&self, item: &Item) -> bool {
        self.status.is_none_or(|s| item.status == s)
            && self
                .owner
                .as_deref()
                .is_none_or(|o| item.owner.as_ref().map(Actor::as_str) == Some(o))
            && self.label.iter().all(|l| item.labels.contains(l))
    }

    /// `--limit N` as a `.take()` bound.
    pub fn take(&self) -> usize {
        self.limit.unwrap_or(usize::MAX)
    }
}

/// A body-source flag pair — `--body FILE|-` reads a file or stdin,
/// `--text` is inline. Mutually exclusive.
#[derive(Debug, Default, clap::Args)]
pub struct BodyFlags {
    /// Body markdown from a file, or `-` for stdin.
    #[arg(long, conflicts_with = "text", value_name = "FILE|-")]
    pub body: Option<String>,
    /// Body markdown inline.
    #[arg(long)]
    pub text: Option<String>,
}

impl BodyFlags {
    /// The supplied body, or `None` → the caller uses the template.
    pub fn read(&self) -> Result<Option<String>, String> {
        if let Some(t) = &self.text {
            return Ok(Some(t.clone()));
        }
        match self.body.as_deref() {
            None => Ok(None),
            Some("-") => {
                use std::io::Read;
                let mut s = String::new();
                std::io::stdin()
                    .read_to_string(&mut s)
                    .map_err(|e| format!("read --body from stdin: {e}"))?;
                Ok(Some(s))
            }
            Some(path) => std::fs::read_to_string(path)
                .map(Some)
                .map_err(|e| format!("read --body {path}: {e}")),
        }
    }
}

/// TODO.md + BACKLOG.md loaded together — task items move between
/// them on status changes (`deferred` parks, `done` deletes).
pub struct Ledger {
    /// `TODO.md`.
    pub todo: TodoDoc,
    /// `BACKLOG.md`.
    pub backlog: BacklogDoc,
}

/// Which doc a task lives in — and its position inside. `path` is
/// the index trail into the section's item tree: `[2]` is the
/// third top-level item, `[2,0]` its first child.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskLoc {
    /// `true` = BACKLOG.md, `false` = TODO.md.
    pub backlog: bool,
    /// Section index.
    pub section: usize,
    /// Index path within the section — len ≥ 1; deeper than 1
    /// means a nested child.
    pub path: Vec<usize>,
}

impl TaskLoc {
    /// Is this a nested child (not a top-level line)?
    pub fn is_child(&self) -> bool {
        self.path.len() > 1
    }
}

impl Ledger {
    /// Load both docs.
    pub fn load(store: &MdStore) -> Result<Self, String> {
        Ok(Ledger {
            todo: store.todo().map_err(|e| e.to_string())?,
            backlog: store.backlog().map_err(|e| e.to_string())?,
        })
    }

    /// Locate a task in either doc — top-level lines and nested
    /// children alike (`None` when it's not a task or doesn't
    /// exist).
    pub fn locate(&self, id: ItemId) -> Option<TaskLoc> {
        find_pos(&self.todo.sections, id)
            .map(|(section, path)| TaskLoc {
                backlog: false,
                section,
                path,
            })
            .or_else(|| {
                find_pos(&self.backlog.sections, id).map(|(section, path)| TaskLoc {
                    backlog: true,
                    section,
                    path,
                })
            })
    }

    /// The item at `loc`.
    pub fn item(&self, loc: &TaskLoc) -> &Item {
        let sections = if loc.backlog {
            &self.backlog.sections
        } else {
            &self.todo.sections
        };
        item_at(&sections[loc.section].items, &loc.path)
    }

    /// Mutable access to the item at `loc`.
    pub fn item_mut(&mut self, loc: &TaskLoc) -> &mut Item {
        let sections = if loc.backlog {
            &mut self.backlog.sections
        } else {
            &mut self.todo.sections
        };
        item_at_mut(&mut sections[loc.section].items, &loc.path)
    }

    /// Remove the item at `loc` — the whole subtree goes with it
    /// (`done` on a parent removes its children too). A section
    /// the removal emptied is dropped (an empty unsectioned run
    /// can't even be emitted).
    pub fn take(&mut self, loc: &TaskLoc) -> Item {
        let sections = if loc.backlog {
            &mut self.backlog.sections
        } else {
            &mut self.todo.sections
        };
        let mut level = &mut sections[loc.section].items;
        for &idx in &loc.path[..loc.path.len() - 1] {
            level = &mut level[idx].children;
        }
        let item = level.remove(loc.path[loc.path.len() - 1]);
        if loc.path.len() == 1 && sections[loc.section].items.is_empty() {
            sections.remove(loc.section);
        }
        item
    }

    /// Swap the item at `loc` with a sibling — `delta` -1 moves up,
    /// +1 down. Works on nested children (siblings share the
    /// parent). Returns false at the list edges.
    pub fn move_within(&mut self, loc: &mut TaskLoc, delta: isize) -> bool {
        let sections = if loc.backlog {
            &mut self.backlog.sections
        } else {
            &mut self.todo.sections
        };
        let mut level = &mut sections[loc.section].items;
        for &idx in &loc.path[..loc.path.len() - 1] {
            level = &mut level[idx].children;
        }
        let i = loc.path[loc.path.len() - 1] as isize;
        let j = i + delta;
        if j < 0 || j as usize >= level.len() {
            return false;
        }
        level.swap(i as usize, j as usize);
        let last = loc.path.len() - 1;
        loc.path[last] = j as usize;
        true
    }

    /// Append to a TODO.md section: `--section` names one (created
    /// when absent); default = the first section, else an
    /// unsectioned run.
    pub fn put_todo(&mut self, item: Item, section: Option<&str>) {
        push_item(&mut self.todo.sections, item, section);
    }

    /// Append to BACKLOG.md — same placement rule as [`put_todo`].
    pub fn put_backlog(&mut self, item: Item, section: Option<&str>) {
        push_item(&mut self.backlog.sections, item, section);
    }

    /// Write both docs back.
    pub fn save(&self, store: &MdStore) -> Result<(), String> {
        store.save_todo(&self.todo).map_err(|e| e.to_string())?;
        store.save_backlog(&self.backlog).map_err(|e| e.to_string())
    }
}

fn find_pos(sections: &[Section], id: ItemId) -> Option<(usize, Vec<usize>)> {
    sections.iter().enumerate().find_map(|(si, s)| {
        let mut path = Vec::new();
        find_path(&s.items, id, &mut path).then_some((si, path))
    })
}

/// Depth-first id search — pushes the index trail onto `path`.
fn find_path(items: &[Item], id: ItemId, path: &mut Vec<usize>) -> bool {
    for (i, item) in items.iter().enumerate() {
        path.push(i);
        if item.id == id || find_path(&item.children, id, path) {
            return true;
        }
        path.pop();
    }
    false
}

fn item_at<'a>(mut items: &'a [Item], path: &[usize]) -> &'a Item {
    for &idx in &path[..path.len() - 1] {
        items = &items[idx].children;
    }
    &items[path[path.len() - 1]]
}

fn item_at_mut<'a>(mut items: &'a mut [Item], path: &[usize]) -> &'a mut Item {
    for &idx in &path[..path.len() - 1] {
        items = &mut items[idx].children;
    }
    &mut items[path[path.len() - 1]]
}

fn push_item(sections: &mut Vec<Section>, item: Item, section: Option<&str>) {
    match section {
        Some(name) => {
            if let Some(s) = sections
                .iter_mut()
                .find(|s| s.name.as_deref() == Some(name))
            {
                s.items.push(item);
            } else {
                sections.push(Section {
                    name: Some(name.to_string()),
                    items: vec![item],
                });
            }
        }
        None => {
            if sections.is_empty() {
                sections.push(Section::unsectioned());
            }
            sections[0].items.push(item);
        }
    }
}

/// The shortest legal path `from → to` through the state machine
/// (BFS over [`Status::transitions`]). Convenience verbs — `done`,
/// `defer`, `promote`, `resolve`, proposal `status`, ADR
/// `supersede` — walk multi-step paths (e.g. `open → in-progress →
/// done`), so every hop stays machine-validated.
pub fn status_path(id: ItemId, from: Status, to: Status) -> Result<Vec<Status>, String> {
    if from == to {
        return Err(format!("{id} is already {to}"));
    }
    let mut prev: BTreeMap<Status, Status> = BTreeMap::new();
    let mut q = VecDeque::from([from]);
    while let Some(cur) = q.pop_front() {
        if cur == to {
            break;
        }
        for &next in cur.transitions() {
            if next != from && !prev.contains_key(&next) {
                prev.insert(next, cur);
                q.push_back(next);
            }
        }
    }
    if !prev.contains_key(&to) {
        return Err(format!(
            "{id}: invalid status transition {from} → {to} (valid from {from}: {})",
            targets(from)
        ));
    }
    let mut path = vec![to];
    while let Some(&cur) = path.last() {
        if cur == from {
            break;
        }
        path.push(prev[&cur]);
    }
    path.pop();
    path.reverse();
    Ok(path)
}

/// `"claimed, in-progress, deferred"` / `"none (terminal)"` — the
/// valid-targets suffix on transition errors.
pub fn targets(from: Status) -> String {
    let t: Vec<&str> = from.transitions().iter().map(|s| s.token()).collect();
    if t.is_empty() {
        "none (terminal)".into()
    } else {
        t.join(", ")
    }
}

/// Walk `item` along [`status_path`] to `to`, validating each hop.
pub fn walk_to(item: &mut Item, to: Status) -> Result<(), String> {
    for step in status_path(item.id, item.status, to)? {
        item.status = item.status.transition_to(step).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Walk every descendant to `to` — subtree moves keep the ledger
/// consistent: a deferred parent's children can't stay open in
/// BACKLOG.md, and children park/unpark with their parent. Nodes
/// already at `to` are left alone (identity isn't a transition).
pub fn walk_descendants_to(item: &mut Item, to: Status) -> Result<(), String> {
    for c in &mut item.children {
        if c.status != to {
            walk_to(c, to)?;
        }
        walk_descendants_to(c, to)?;
    }
    Ok(())
}

/// Verbs are kind-locked — `myco issue resolve T-001` is a usage
/// error, not a lookup failure.
pub fn require_kind(id: ItemId, kind: ItemKind, verb: &str) -> Result<(), String> {
    if id.kind() == kind {
        Ok(())
    } else {
        Err(format!(
            "{verb} takes {}-NNN ids — {id} is a {} item",
            kind.prefix(),
            id.kind().prefix()
        ))
    }
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

/// Resolve the project + open the store — every item verb's start.
pub fn open(
    dir: Option<&Path>,
    root: Option<&Path>,
) -> Result<(discover::Project, MdStore), String> {
    let proj = discover::discover(dir, root)?;
    let store = ops::store_at(&proj.worktree)?;
    Ok((proj, store))
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

/// `T-001 open pri:high owner:devin #core title` — the compact
/// human list line.
pub fn compact(item: &Item) -> String {
    let mut s = format!("{} {} pri:{}", item.id, item.status, item.priority.token());
    if let Some(o) = &item.owner {
        s += &format!(" owner:{o}");
    }
    for l in &item.labels {
        s += &format!(" #{l}");
    }
    if let Some(e) = &item.epic {
        s += &format!(" epic:{e}");
    }
    if let Some(m) = &item.milestone {
        s += &format!(" ms:{m}");
    }
    if !item.title.is_empty() {
        s += &format!(" {}", item.title);
    }
    s
}

/// `--json` → a bare array of item objects; else compact lines,
/// paged when tall.
pub fn print_list(items: &[Item], json_out: bool) -> Result<i32, String> {
    if json_out {
        let v: Vec<serde_json::Value> = items
            .iter()
            .map(|i| serde_json::to_value(i).unwrap_or(json!(null)))
            .collect();
        ops::print_json(&json!(v));
    } else {
        let out: Vec<String> = items.iter().map(compact).collect();
        ops::print_paged(&out.join("\n"));
    }
    Ok(0)
}

/// Depth-tagged pre-order view — task list verbs indent children
/// under their parent by it.
pub fn tree_view(items: &[Item]) -> Vec<(usize, &Item)> {
    fn go<'a>(item: &'a Item, depth: usize, out: &mut Vec<(usize, &'a Item)>) {
        out.push((depth, item));
        for c in &item.children {
            go(c, depth + 1, out);
        }
    }
    let mut out = Vec::new();
    for i in items {
        go(i, 0, &mut out);
    }
    out
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
        let out: Vec<String> = pairs
            .iter()
            .map(|(depth, i)| format!("{}{}", "  ".repeat(*depth), compact(i)))
            .collect();
        ops::print_paged(&out.join("\n"));
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
    /// `--body/--text` flags — the template fills in when absent.
    pub body: &'a BodyFlags,
    /// `--pri/--owner/--label/--blocks` flags.
    pub meta: &'a MetaFlags,
    /// The kind's default body template.
    pub template: fn(&Item) -> String,
    /// Commit-message noun — `chore: new <word> <id>`.
    pub word: &'a str,
    /// `--json` output.
    pub json_out: bool,
    /// `--no-commit`.
    pub no_commit: bool,
}

/// The shared `<kind> new` body — alloc id → apply meta flags →
/// body from `--body/--text` or `template` → `save_item` → commit
/// + report.
pub fn new_file(
    proj: &discover::Project,
    store: &MdStore,
    spec: &NewItem<'_>,
) -> Result<i32, String> {
    let id = store.alloc_id(spec.kind).map_err(|e| e.to_string())?;
    let mut item = Item::new(id, &spec.title).created(today());
    spec.meta.apply(&mut item)?;
    let body = spec.body.read()?.unwrap_or_else(|| (spec.template)(&item));
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

/// The shared `<kind> list` — live item files, plus `archive/`
/// entries when `include_archived`.
pub fn list_files(
    store: &MdStore,
    kind: ItemKind,
    include_archived: bool,
    list: &ListFlags,
    json_out: bool,
) -> Result<i32, String> {
    let mut found = store.list_items(kind).map_err(|e| e.to_string())?;
    if include_archived {
        found.extend(store.list_archived(kind).map_err(|e| e.to_string())?);
    }
    let found: Vec<Item> = found
        .into_iter()
        .map(|f| f.meta)
        .filter(|i| list.matches(i))
        .take(list.take())
        .collect();
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
    let ledger = Ledger::load(store)?;
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

    #[test]
    fn status_path_walks() {
        let id = ItemId::new(ItemKind::Task, 1);
        assert_eq!(
            status_path(id, Status::Open, Status::Done).unwrap(),
            vec![Status::InProgress, Status::Done]
        );
        assert_eq!(
            status_path(id, Status::Open, Status::Deferred).unwrap(),
            vec![Status::Deferred]
        );
        assert_eq!(
            status_path(id, Status::Blocked, Status::Done).unwrap(),
            vec![Status::InProgress, Status::Done]
        );
        // deferred reaches done only via open → in-progress
        assert_eq!(
            status_path(id, Status::Deferred, Status::Done).unwrap(),
            vec![Status::Open, Status::InProgress, Status::Done]
        );
        // no path out of terminal states
        assert!(status_path(id, Status::Done, Status::Open).is_err());
        // same → error
        assert!(status_path(id, Status::Open, Status::Open).is_err());
        // error message lists valid targets
        let e = status_path(id, Status::Done, Status::Open).unwrap_err();
        assert!(e.contains("none (terminal)"), "{e}");
    }

    #[test]
    fn compact_line() {
        let i = Item::new(ItemId::new(ItemKind::Task, 1), "the thing")
            .priority(Priority::High)
            .owner(Actor::new("devin").unwrap())
            .label("core")
            .epic("v2")
            .milestone("1.0");
        assert_eq!(
            compact(&i),
            "T-001 open pri:high owner:devin #core epic:v2 ms:1.0 the thing"
        );
        let bare = Item::new(ItemId::new(ItemKind::Task, 2), "");
        assert_eq!(compact(&bare), "T-002 open pri:med");
    }

    #[test]
    fn ledger_moves_and_prunes() {
        let dir = tempfile::tempdir().unwrap();
        let root = camino::Utf8PathBuf::from_path_buf(dir.path().join("p")).unwrap();
        std::fs::create_dir_all(&root).unwrap();
        let store = MdStore::new(root);
        let mut ledger = Ledger::load(&store).unwrap();
        let id = ItemId::new(ItemKind::Task, 1);
        ledger.put_todo(Item::new(id, "x"), None);
        ledger.put_todo(Item::new(ItemId::new(ItemKind::Task, 2), "y"), Some("Now"));
        let loc = ledger.locate(id).unwrap();
        assert!(!loc.backlog);
        let item = ledger.take(&loc);
        assert_eq!(item.id, id);
        // emptied unsectioned run dropped
        assert!(ledger.todo.sections.iter().all(|s| !s.items.is_empty()));
        assert!(ledger.locate(id).is_none());
        ledger.put_backlog(item.status(Status::Deferred), None);
        let loc = ledger.locate(id).unwrap();
        assert!(loc.backlog);
        ledger.save(&store).unwrap();
        // round-trip through the real store
        let reloaded = Ledger::load(&store).unwrap();
        assert!(reloaded.locate(id).is_some());
    }

    #[test]
    fn ledger_locates_and_takes_children() {
        let dir = tempfile::tempdir().unwrap();
        let root = camino::Utf8PathBuf::from_path_buf(dir.path().join("p")).unwrap();
        std::fs::create_dir_all(&root).unwrap();
        let store = MdStore::new(root);
        let t = |n: u32| Item::new(ItemId::new(ItemKind::Task, n), format!("t{n}"));
        let mut ledger = Ledger::load(&store).unwrap();
        ledger.put_todo(t(1).child(t(2).child(t(4))).child(t(3)), None);
        ledger.put_todo(t(5), None);
        // locate walks the whole tree — the path says where
        let loc = ledger.locate(ItemId::new(ItemKind::Task, 4)).unwrap();
        assert_eq!(loc.path, vec![0, 0, 0]);
        assert!(loc.is_child());
        assert_eq!(ledger.item(&loc).id.number(), 4);
        // take on a child removes just that node
        ledger.take(&loc);
        let parent = &ledger.todo.sections[0].items[0];
        assert_eq!(parent.children[0].children.len(), 0);
        // take on a parent removes the whole subtree; sections
        // stay (a sibling remains)
        let loc = ledger.locate(ItemId::new(ItemKind::Task, 1)).unwrap();
        let gone = ledger.take(&loc);
        assert_eq!(gone.children.len(), 2);
        assert!(ledger.locate(ItemId::new(ItemKind::Task, 2)).is_none());
        assert_eq!(ledger.todo.sections[0].items.len(), 1);
        // mutation in place reaches children too
        let loc = ledger.locate(ItemId::new(ItemKind::Task, 5)).unwrap();
        ledger.item_mut(&loc).children.push(t(6));
        assert!(ledger.locate(ItemId::new(ItemKind::Task, 6)).is_some());
        ledger.save(&store).unwrap();
        assert!(Ledger::load(&store)
            .unwrap()
            .locate(ItemId::new(ItemKind::Task, 6))
            .is_some());
    }

    #[test]
    fn move_within_swaps_siblings() {
        let dir = tempfile::tempdir().unwrap();
        let root = camino::Utf8PathBuf::from_path_buf(dir.path().join("p")).unwrap();
        std::fs::create_dir_all(&root).unwrap();
        let store = MdStore::new(root);
        let t = |n: u32| Item::new(ItemId::new(ItemKind::Task, n), format!("t{n}"));
        let mut ledger = Ledger::load(&store).unwrap();
        ledger.put_todo(t(1), None);
        ledger.put_todo(t(2), None);
        ledger.put_todo(t(3).child(t(4)), None);
        // edges don't move
        let mut loc = ledger.locate(ItemId::new(ItemKind::Task, 1)).unwrap();
        assert!(!ledger.move_within(&mut loc, -1));
        // down swaps with the next sibling and updates loc
        assert!(ledger.move_within(&mut loc, 1));
        assert_eq!(loc.path, vec![1]);
        assert_eq!(ledger.todo.sections[0].items[1].id.number(), 1);
        // nested children reorder among themselves
        let mut loc = ledger.locate(ItemId::new(ItemKind::Task, 4)).unwrap();
        assert!(!ledger.move_within(&mut loc, -1));
        ledger.save(&store).unwrap();
        let reloaded = Ledger::load(&store).unwrap();
        assert_eq!(reloaded.todo.sections[0].items[1].id.number(), 1);
    }

    #[test]
    fn walk_descendants_skips_already_there() {
        let t = |n: u32, s: Status| Item::new(ItemId::new(ItemKind::Task, n), "x").status(s);
        let mut parent = t(1, Status::Open).child(t(2, Status::InProgress));
        walk_descendants_to(&mut parent, Status::Deferred).unwrap();
        assert_eq!(parent.children[0].status, Status::Deferred);
        // already-deferred nodes don't re-error on the identity walk
        let mut parked = t(3, Status::Deferred).child(t(4, Status::Deferred));
        walk_descendants_to(&mut parked, Status::Deferred).unwrap();
        // unpark: deferred children come back to open
        walk_descendants_to(&mut parked, Status::Open).unwrap();
        assert_eq!(parked.children[0].status, Status::Open);
    }

    #[test]
    fn tree_view_tags_depth() {
        let t = |n: u32| Item::new(ItemId::new(ItemKind::Task, n), format!("t{n}"));
        let items = vec![t(1).child(t(2).child(t(4))).child(t(3)), t(5)];
        let flat: Vec<(usize, u32)> = tree_view(&items)
            .into_iter()
            .map(|(d, i)| (d, i.id.number()))
            .collect();
        assert_eq!(flat, [(0, 1), (1, 2), (2, 4), (1, 3), (0, 5)]);
    }

    #[test]
    fn require_kind_message() {
        let t = ItemId::new(ItemKind::Task, 1);
        assert!(require_kind(t, ItemKind::Task, "todo").is_ok());
        let i = ItemId::new(ItemKind::Issue, 1);
        let e = require_kind(i, ItemKind::Task, "todo").unwrap_err();
        assert!(e.contains("T-NNN"), "{e}");
    }
}
