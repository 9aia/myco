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

/// `--pri/--owner/--label/--blocks` — the metadata flags shared by
/// every `add`/`new` verb.
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

/// Which doc a task lives in — and its position inside.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskLoc {
    /// `true` = BACKLOG.md, `false` = TODO.md.
    pub backlog: bool,
    /// Section index.
    pub section: usize,
    /// Item index within the section.
    pub item: usize,
}

impl Ledger {
    /// Load both docs.
    pub fn load(store: &MdStore) -> Result<Self, String> {
        Ok(Ledger {
            todo: store.todo().map_err(|e| e.to_string())?,
            backlog: store.backlog().map_err(|e| e.to_string())?,
        })
    }

    /// Locate a task in either doc (`None` when it's not a task or
    /// doesn't exist).
    pub fn locate(&self, id: ItemId) -> Option<TaskLoc> {
        find_pos(&self.todo.sections, id)
            .map(|(section, item)| TaskLoc {
                backlog: false,
                section,
                item,
            })
            .or_else(|| {
                find_pos(&self.backlog.sections, id).map(|(section, item)| TaskLoc {
                    backlog: true,
                    section,
                    item,
                })
            })
    }

    /// The item at `loc`.
    pub fn item(&self, loc: TaskLoc) -> &Item {
        let sections = if loc.backlog {
            &self.backlog.sections
        } else {
            &self.todo.sections
        };
        &sections[loc.section].items[loc.item]
    }

    /// Mutable access to the item at `loc`.
    pub fn item_mut(&mut self, loc: TaskLoc) -> &mut Item {
        let sections = if loc.backlog {
            &mut self.backlog.sections
        } else {
            &mut self.todo.sections
        };
        &mut sections[loc.section].items[loc.item]
    }

    /// Remove the item at `loc`; a section the removal emptied is
    /// dropped (an empty unsectioned run can't even be emitted).
    pub fn take(&mut self, loc: TaskLoc) -> Item {
        let sections = if loc.backlog {
            &mut self.backlog.sections
        } else {
            &mut self.todo.sections
        };
        let item = sections[loc.section].items.remove(loc.item);
        if sections[loc.section].items.is_empty() {
            sections.remove(loc.section);
        }
        item
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

fn find_pos(sections: &[Section], id: ItemId) -> Option<(usize, usize)> {
    sections
        .iter()
        .enumerate()
        .find_map(|(si, s)| s.items.iter().position(|i| i.id == id).map(|ii| (si, ii)))
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
    if !item.title.is_empty() {
        s += &format!(" {}", item.title);
    }
    s
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
        return Ok(Some((ledger.item(loc).clone(), file.to_string(), None)));
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
            .label("core");
        assert_eq!(
            compact(&i),
            "T-001 open pri:high owner:devin #core the thing"
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
        let item = ledger.take(loc);
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
    fn require_kind_message() {
        let t = ItemId::new(ItemKind::Task, 1);
        assert!(require_kind(t, ItemKind::Task, "todo").is_ok());
        let i = ItemId::new(ItemKind::Issue, 1);
        let e = require_kind(i, ItemKind::Task, "todo").unwrap_err();
        assert!(e.contains("T-NNN"), "{e}");
    }
}
