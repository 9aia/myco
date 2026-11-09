//! `myco item show|set` — the kind-agnostic item verbs: locate an
//! item anywhere in the ledger; mutate metadata, with `--status`
//! going through the transition machine only.

use myco_core::{is_valid_label, Actor, EdgeKind, Item, ItemId, Priority, Status};
use myco_store::Store;
use serde_json::json;

use crate::items;
use crate::registry::{Args, Ctx};

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

/// The decoded `item set` flags — shared by the CLI handler and
/// the wire handler (`myco serve`'s `item.set`).
pub(crate) fn set_flags(a: &Args) -> Result<SetFlags, String> {
    Ok(SetFlags {
        status: a.parsed("status")?,
        pri: a.parsed("pri")?,
        owner: a.parsed("owner")?,
        label: a.many("label"),
        unlabel: a.many("unlabel"),
        blocks: a.many("blocks"),
        unblock: a.many_parsed("unblock")?,
        epic: a.opt("epic").map(str::to_string),
        milestone: a.opt("milestone").map(str::to_string),
    })
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

/// The `item show` verb.
pub fn h_show(ctx: &Ctx, a: &Args) -> Result<i32, String> {
    let (_proj, store) = items::open_read(ctx.dir.as_deref(), ctx.root.as_deref())?;
    show(&store, &a.many_parsed::<ItemId>("id")?, ctx.json)
}

/// The link graph behind the related section — derived live, not
/// read from the persisted index. One gather per `item show` call,
/// shared across a multi-id batch (see [`myco_ops::links::live_graph`]
/// for why the index isn't consulted here). The gather is
/// per-source lenient — a corrupt file costs its own edges — and
/// its warnings go to stderr: they're drift `check` reports, not
/// show's output.
fn graph(store: &impl Store) -> myco_ops::links::LiveGraph {
    let (graph, warnings) = myco_ops::links::live_graph(store);
    for w in warnings {
        eprintln!("myco: warning: {w}");
    }
    graph
}

/// `item set` + an optional `--session` claim tag — the shared
/// mutation path when absent, [`crate::continuity::tag_claim`]
/// (mutation + `claimed` entry, atomically) when present.
/// Returns the post-mutation item, the location label, and the
/// claim entry when one was written.
pub(crate) fn set_item(
    store: &impl Store,
    id: ItemId,
    flags: &SetFlags,
    session: Option<String>,
) -> Result<(Option<Item>, String, Option<myco_core::HandoffEntry>), String> {
    match session {
        Some(s) => {
            let actor = flags.owner.clone();
            let (item, location, entry) =
                crate::continuity::tag_claim(store, id, flags, actor, &s)?;
            Ok((item, location, Some(entry)))
        }
        None => {
            let (item, location) = mutate_item(store, id, flags)?;
            Ok((item, location, None))
        }
    }
}

/// The `item set` verb.
pub fn h_set(ctx: &Ctx, a: &Args) -> Result<i32, String> {
    let id = a.req::<ItemId>("id")?;
    let flags = set_flags(a)?;
    let session = a.opt("session").map(str::to_string);
    if flags.is_noop() && session.is_none() {
        return Err("item set: nothing to change — pass at least one flag".into());
    }
    // done is terminal — the one `item set` worth confirming
    if flags.status == Some(Status::Done)
        && crate::confirm::needed(a.flag("yes"))
        && !crate::confirm::ask(&format!("set {id} → done? done is terminal."))
    {
        eprintln!("myco: cancelled");
        return Ok(1);
    }
    let (proj, store) = items::open(ctx.dir.as_deref(), ctx.root.as_deref())?;
    let (item, location, claim) = set_item(&store, id, &flags, session)?;
    let committed = items::commit(
        &proj.worktree,
        &format!("chore: set {id}"),
        flags.owner.as_ref(),
        ctx.no_commit,
    )?;
    let human = item
        .as_ref()
        .map(|i| format!("set {}", items::compact(i)))
        .unwrap_or_else(|| format!("set {id} done — line removed"));
    let mut payload = json!({"item": item, "location": location});
    if let Some(entry) = claim {
        payload["claim"] = json!(entry);
    }
    items::report(payload, committed, ctx.json, &human)
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
    /// The item's link-graph neighborhood — kind already
    /// mirrored for incoming edges (`blocked_by`, …).
    related: Vec<myco_ops::links::RelatedItem>,
}

/// Load one id anywhere in the ledger — TODO/BACKLOG line or
/// item file, live or archived — plus its related edges out of
/// `graph`.
fn show_data(
    store: &impl Store,
    id: ItemId,
    graph: &myco_ops::links::LiveGraph,
) -> Result<ShowData, String> {
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
                related: graph.related(id),
            })
        }
        None => Err(format!("{id}: no such item")),
    }
}

/// The related section's human lines — the TUI's preview shape:
/// one `kind:` line per edge kind in declaration order, neighbors
/// joined by `·`, `ID title` when the ledger knows the other end
/// (a `handoff` self-edge prints the file path instead of the
/// item's own id). Empty graph → empty string.
fn related_lines(related: &[myco_ops::links::RelatedItem]) -> String {
    let mut out = String::from("\n  related:");
    for kind in EdgeKind::ALL {
        let dsts: Vec<String> = related
            .iter()
            .filter(|r| r.kind == kind)
            .map(|r| {
                if kind == EdgeKind::Handoff {
                    return format!("handoffs/{}.md", r.other);
                }
                match &r.title {
                    Some(t) if !t.is_empty() => format!("{} {}", r.other, t),
                    _ => r.other.to_string(),
                }
            })
            .collect();
        if !dsts.is_empty() {
            out.push_str(&format!("\n    {:<13} {}", kind.name(), dsts.join(" · ")));
        }
    }
    out
}

/// The related section's JSON shape — `{kind: [{id, title?}]}`,
/// kinds in declaration order, empty groups omitted, `title`
/// only when the ledger resolves the neighbor.
fn related_json(related: &[myco_ops::links::RelatedItem]) -> serde_json::Value {
    let mut groups = serde_json::Map::new();
    for kind in EdgeKind::ALL {
        let items: Vec<serde_json::Value> = related
            .iter()
            .filter(|r| r.kind == kind)
            .map(|r| {
                let mut v = json!({"id": r.other.to_string()});
                if let Some(t) = &r.title {
                    v["title"] = json!(t);
                }
                v
            })
            .collect();
        if !items.is_empty() {
            groups.insert(kind.name().to_string(), json!(items));
        }
    }
    serde_json::Value::Object(groups)
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
    if !d.related.is_empty() {
        out.push_str(&related_lines(&d.related));
    }
    if let Some(body) = &d.body {
        if !body.is_empty() {
            out.push_str(&format!("\n\n{body}"));
        }
    }
    out
}

/// `ShowData` → the `{item, location, handoffs, interrupted,
/// related, body}` payload shape.
fn show_value(d: &ShowData) -> serde_json::Value {
    json!({"item": d.item, "location": d.location, "handoffs": d.handoffs, "interrupted": d.interrupted, "related": related_json(&d.related), "body": d.body})
}

/// The `item show` payload — `{item, location, handoffs,
/// interrupted, related, body}` — the `myco serve` method surface
/// (related-ness rides `item.show`; `batch` inherits it).
pub(crate) fn show_json(store: &impl Store, id: ItemId) -> Result<serde_json::Value, String> {
    let graph = graph(store);
    Ok(show_value(&show_data(store, id, &graph)?))
}

/// The `item show` body — 1..n ids in one open. Single id keeps
/// the classic block + bare payload; multi-id repeats the block
/// in argument order (a bad id reports to stderr, exit 1) and
/// `--json` becomes the `[{id, ok, payload|error}]` array.
fn show(store: &impl Store, ids: &[ItemId], json_out: bool) -> Result<i32, String> {
    let graph = graph(store);
    if ids.len() == 1 {
        let d = show_data(store, ids[0], &graph)?;
        if json_out {
            crate::ops::print_json(&show_value(&d));
        } else {
            println!("{}", show_block(&d));
        }
        return Ok(0);
    }
    if json_out {
        let (entries, failed) = items::multi_json(ids, |id| {
            show_data(store, id, &graph).map(|d| show_value(&d))
        });
        crate::ops::print_json(&entries);
        return Ok(if failed { 1 } else { 0 });
    }
    let mut blocks = Vec::new();
    let mut failed = false;
    for id in ids {
        match show_data(store, *id, &graph) {
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
    store: &impl Store,
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
        let mut done_doc = None;
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
                    let mut item = ledger
                        .take(&loc)
                        .ok_or_else(|| format!("{id}: no such task"))?;
                    // DONE.md emit is done-only — archive the
                    // whole subtree done, like `todo done`
                    items::walk_descendants_to(&mut item, Status::Done)?;
                    // stage before any write — a dup/parse
                    // failure leaves the ledger untouched
                    let mut doc = items::load_done(store)?;
                    items::done_push(&mut doc, item)?;
                    done_doc = Some(doc);
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
        if let Some(doc) = done_doc {
            items::save_done(store, &doc)?;
        }
        Ok((item, "tasks".to_string()))
    } else {
        let mut file = items::item_for_edit(store, id)?;
        apply_set(&mut file.meta, flags)?;
        let item = file.meta.clone();
        store.save_item(&file).map_err(|e| e.to_string())?;
        let location = items::item_rel_path(store, &item)?;
        Ok((Some(item), location))
    }
}
