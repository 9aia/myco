//! `myco item show|set` — the kind-agnostic item verbs: locate an
//! item anywhere in the ledger; mutate metadata, with `--status`
//! going through the transition machine only.

use std::path::PathBuf;

use clap::Subcommand;
use myco_core::{is_valid_label, Actor, Item, ItemId, Priority, Status};
use myco_store::Store;
use serde_json::json;

use crate::items;

#[derive(Subcommand)]
pub enum Cmd {
    /// Dump items — frontmatter fields plus the body for
    /// file-backed kinds (`--json` = the full Item + location +
    /// body). 1..n ids: a batch repeats the block in argument
    /// order; `--json` emits the per-id `[{id, ok, …}]` array.
    Show {
        /// Any item id (`T-NNN`, `I-NNN`, `P-NNN`, `ADR-NNNN`,
        /// `PL-NNN`) — live or archived.
        #[arg(required = true, num_args = 1..)]
        id: Vec<ItemId>,
    },
    /// Mutate an item in place. `--status` is the only field routed
    /// through the state machine — single steps only, invalid ones
    /// error with the valid targets. Status changes on tasks move
    /// the line between TODO.md and BACKLOG.md (`done` deletes it).
    Set {
        /// Any item id.
        id: ItemId,
        /// New status — one legal step from the current one.
        #[arg(long)]
        status: Option<Status>,
        /// Priority: low|med|high|crit.
        #[arg(long, visible_alias = "priority")]
        pri: Option<Priority>,
        /// Owner actor id.
        #[arg(long)]
        owner: Option<Actor>,
        /// Add (`+x` or `x`) or remove (`-x`) a label — repeatable.
        #[arg(long, allow_hyphen_values = true)]
        label: Vec<String>,
        /// Remove a label — repeatable.
        #[arg(long)]
        unlabel: Vec<String>,
        /// Add (`+T-1` or `T-1`) or remove (`-T-1`) a blocks dep —
        /// repeatable or comma-separated.
        #[arg(long, allow_hyphen_values = true, value_delimiter = ',')]
        blocks: Vec<String>,
        /// Remove a blocks dep — repeatable or comma-separated.
        #[arg(long, value_delimiter = ',')]
        unblock: Vec<ItemId>,
        /// Epic name — `""` clears the epic.
        #[arg(long)]
        epic: Option<String>,
        /// Milestone name — `""` clears the milestone.
        #[arg(long)]
        milestone: Option<String>,
    },
}

/// The set-verb flag bundle — applied to an `Item` after the
/// status transition check.
#[derive(Default)]
pub(crate) struct SetFlags {
    pub(crate) status: Option<Status>,
    pub(crate) pri: Option<Priority>,
    pub(crate) owner: Option<Actor>,
    pub(crate) label: Vec<String>,
    pub(crate) unlabel: Vec<String>,
    pub(crate) blocks: Vec<String>,
    pub(crate) unblock: Vec<ItemId>,
    pub(crate) epic: Option<String>,
    pub(crate) milestone: Option<String>,
}

impl SetFlags {
    /// A status-only mutation — `myco interrupt` drives a legal
    /// step through the same relocation path as `item set`.
    pub(crate) fn status(to: Status) -> Self {
        SetFlags {
            status: Some(to),
            ..Default::default()
        }
    }

    pub(crate) fn is_noop(&self) -> bool {
        self.status.is_none()
            && self.pri.is_none()
            && self.owner.is_none()
            && self.label.is_empty()
            && self.unlabel.is_empty()
            && self.blocks.is_empty()
            && self.unblock.is_empty()
            && self.epic.is_none()
            && self.milestone.is_none()
    }
}

/// `+x` → add `x`, `-x` → remove `x`, `x` → add — the `item set`
/// add/remove convention.
fn signed(s: &str) -> (bool, &str) {
    match s.strip_prefix('-') {
        Some(rest) => (false, rest),
        None => (true, s.strip_prefix('+').unwrap_or(s)),
    }
}

/// Apply every set flag to `item` — status first (machine-checked),
/// then the plain metadata.
fn apply_set(item: &mut Item, flags: &SetFlags) -> Result<(), String> {
    if let Some(to) = flags.status {
        let from = item.status;
        item.status = from.transition_to(to).map_err(|_| {
            format!(
                "{id}: invalid status transition {from} → {to} (valid from {from}: {t})",
                id = item.id,
                t = items::targets(from)
            )
        })?;
    }
    if let Some(p) = flags.pri {
        item.priority = p;
    }
    if let Some(o) = &flags.owner {
        item.owner = Some(o.clone());
    }
    for raw in &flags.label {
        let (add, name) = signed(raw);
        if !is_valid_label(name) {
            return Err(format!("invalid label {name:?}"));
        }
        if add {
            item.labels.insert(name.to_string());
        } else {
            item.labels.remove(name);
        }
    }
    for name in &flags.unlabel {
        item.labels.remove(name.as_str());
    }
    for raw in &flags.blocks {
        let (add, s) = signed(raw);
        let id: ItemId = s
            .parse()
            .map_err(|e: myco_core::ParseItemIdError| e.to_string())?;
        if add {
            item.blocks.insert(id);
        } else {
            item.blocks.remove(&id);
        }
    }
    for id in &flags.unblock {
        item.blocks.remove(id);
    }
    for (what, flag, slot) in [
        ("epic", &flags.epic, &mut item.epic),
        ("milestone", &flags.milestone, &mut item.milestone),
    ] {
        if let Some(name) = flag {
            if name.is_empty() {
                *slot = None; // `--epic ""` clears
            } else if !myco_core::is_valid_group_name(name) {
                return Err(format!("invalid {what} name {name:?}"));
            } else {
                *slot = Some(name.clone());
            }
        }
    }
    Ok(())
}

/// The `item` verb group.
pub fn run(
    cmd: Cmd,
    dir: Option<PathBuf>,
    root: Option<PathBuf>,
    json_out: bool,
    no_commit: bool,
) -> Result<i32, String> {
    let (proj, store) = if matches!(cmd, Cmd::Show { .. }) {
        items::open_read(dir.as_deref(), root.as_deref())?
    } else {
        items::open(dir.as_deref(), root.as_deref())?
    };
    match cmd {
        Cmd::Show { id: ids } => show(&store, &ids, json_out),
        Cmd::Set {
            id,
            status,
            pri,
            owner,
            label,
            unlabel,
            blocks,
            unblock,
            epic,
            milestone,
        } => {
            let flags = SetFlags {
                status,
                pri,
                owner,
                label,
                unlabel,
                blocks,
                unblock,
                epic,
                milestone,
            };
            if flags.is_noop() {
                return Err("item set: nothing to change — pass at least one flag".into());
            }
            let (item, location) = mutate_item(&store, id, &flags)?;
            let committed = items::commit(&proj.worktree, &format!("chore: set {id}"), no_commit)?;
            let human = item
                .as_ref()
                .map(|i| format!("set {}", items::compact(i)))
                .unwrap_or_else(|| format!("set {id} done — line removed"));
            items::report(
                json!({"item": item, "location": location}),
                committed,
                json_out,
                &human,
            )
        }
    }
}

/// The gathered `item show` pieces — one id's load shared by
/// the human block, the `--json` payload, and the multi-id
/// batch (`myco serve` rides [`show_json`]).
struct ShowData {
    item: Item,
    location: String,
    handoffs: usize,
    interrupted: bool,
    body: Option<String>,
}

/// Load one id anywhere in the ledger — TODO/BACKLOG line or
/// item file, live or archived.
fn show_data(store: &myco_store_md::MdStore, id: ItemId) -> Result<ShowData, String> {
    match items::find_item(store, id)? {
        Some((item, location, body)) => {
            let handoffs = store.read_handoffs(id).map_err(|e| e.to_string())?.len();
            let interrupted = crate::continuity::is_interrupted(store, id)?;
            Ok(ShowData {
                item,
                location,
                handoffs,
                interrupted,
                body,
            })
        }
        None => Err(format!("{id}: no such item")),
    }
}

/// One item's human block — identical for single- and multi-id
/// output (blocks join on a blank line).
fn show_block(d: &ShowData) -> String {
    let mut out = format!(
        "{}\n  {}\n  handoffs: {}",
        items::compact(&d.item),
        d.location,
        d.handoffs
    );
    if d.interrupted {
        out.push_str(&format!(
            "\n  ⚡ interrupted — latest handoff marks it; `myco resume {}`",
            d.item.id
        ));
    }
    if let Some(body) = &d.body {
        if !body.is_empty() {
            out.push_str(&format!("\n\n{body}"));
        }
    }
    out
}

/// `ShowData` → the `{item, location, handoffs, interrupted,
/// body}` payload shape.
fn show_value(d: &ShowData) -> serde_json::Value {
    json!({"item": d.item, "location": d.location, "handoffs": d.handoffs, "interrupted": d.interrupted, "body": d.body})
}

/// The `item show` payload — `{item, location, handoffs,
/// interrupted, body}` — the `myco serve` method surface.
pub(crate) fn show_json(
    store: &myco_store_md::MdStore,
    id: ItemId,
) -> Result<serde_json::Value, String> {
    Ok(show_value(&show_data(store, id)?))
}

/// The `item show` body — 1..n ids in one open. Single id keeps
/// the classic block + bare payload; multi-id repeats the block
/// in argument order (a bad id reports to stderr, exit 1) and
/// `--json` becomes the `[{id, ok, payload|error}]` array.
fn show(store: &myco_store_md::MdStore, ids: &[ItemId], json_out: bool) -> Result<i32, String> {
    if ids.len() == 1 {
        let d = show_data(store, ids[0])?;
        if json_out {
            crate::ops::print_json(&show_value(&d));
        } else {
            println!("{}", show_block(&d));
        }
        return Ok(0);
    }
    if json_out {
        let (entries, failed) = items::multi_json(ids, |id| show_json(store, id));
        crate::ops::print_json(&entries);
        return Ok(if failed { 1 } else { 0 });
    }
    let mut blocks = Vec::new();
    let mut failed = false;
    for id in ids {
        match show_data(store, *id) {
            Ok(d) => blocks.push(show_block(&d)),
            Err(e) => {
                failed = true;
                eprintln!("myco: error: {e}");
            }
        }
    }
    if !blocks.is_empty() {
        println!("{}", blocks.join("\n\n"));
    }
    Ok(if failed { 1 } else { 0 })
}

/// Re-land a `take`n top-level task in its origin section: the
/// same-named section when it still exists (empty sections are
/// dropped by `take`, so a `None` name re-matches the surviving
/// unsectioned run at index 0), else re-created at its old index
/// so the item can't drift into `sections[0]` under a heading it
/// never lived under.
fn put_back(
    sections: &mut Vec<myco_core::Section>,
    section: usize,
    name: Option<String>,
    item: Item,
) {
    match sections.iter_mut().find(|s| s.name == name) {
        Some(s) => s.items.push(item),
        None => sections.insert(
            section.min(sections.len()),
            myco_core::Section {
                name,
                items: vec![item],
            },
        ),
    }
}

/// Apply `flags` to one item anywhere in the ledger and save —
/// the shared mutation behind `item set` and `interrupt`. Writes
/// but does NOT commit; the caller owns the commit message.
/// Returns the post-mutation item (`None` when done removed a task
/// line) and a location label for reporting.
pub(crate) fn mutate_item(
    store: &myco_store_md::MdStore,
    id: ItemId,
    flags: &SetFlags,
) -> Result<(Option<Item>, String), String> {
    let mut ledger = items::load_ledger(store)?;
    if let Some(loc) = ledger.locate(id) {
        // task item: mutate in place; a status change may
        // relocate the line — done deletes the subtree,
        // deferred detaches the node into BACKLOG.md
        // (children park with it), an active status moves
        // backlog nodes into TODO.md (children unpark).
        // Nested children that keep their doc stay nested.
        let status_change = flags.status.is_some();
        let Some(target) = ledger.item_mut(&loc) else {
            return Err(format!("{id}: no such task"));
        };
        apply_set(target, flags)?;
        if status_change {
            let Some(current) = ledger.item(&loc) else {
                return Err(format!("{id}: no such task"));
            };
            match current.status {
                Status::Done => {
                    ledger.take(&loc);
                }
                Status::Deferred => {
                    let mut item = ledger
                        .take(&loc)
                        .ok_or_else(|| format!("{id}: no such task"))?;
                    items::walk_descendants_to(&mut item, Status::Deferred)?;
                    ledger.put_backlog(item, None);
                }
                _ if loc.backlog => {
                    let mut item = ledger
                        .take(&loc)
                        .ok_or_else(|| format!("{id}: no such task"))?;
                    items::walk_descendants_to(&mut item, Status::Open)?;
                    ledger.put_todo(item, None);
                }
                _ if !loc.is_child() => {
                    // top-level items re-land at the tail of
                    // their *origin* section — carry its index +
                    // name through the take (an emptied section
                    // is dropped, so re-find/re-create by name)
                    let section = loc.section;
                    let name = ledger
                        .todo
                        .sections
                        .get(section)
                        .and_then(|s| s.name.clone());
                    let Some(item) = ledger.take(&loc) else {
                        return Err(format!("{id}: no such task"));
                    };
                    put_back(&mut ledger.todo.sections, section, name, item);
                }
                _ => {} // a TODO child stays nested
            }
        }
        let item = ledger.locate(id).and_then(|l| ledger.item(&l).cloned());
        items::save_ledger(store, &ledger)?;
        Ok((item, "tasks".to_string()))
    } else {
        let mut file = items::item_for_edit(store, id)?;
        apply_set(&mut file.meta, flags)?;
        let item = file.meta.clone();
        store.save_item(&file).map_err(|e| e.to_string())?;
        let location = items::item_rel_path(&item);
        Ok((Some(item), location))
    }
}
