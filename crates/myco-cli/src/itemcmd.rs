//! `myco item show|set` — the kind-agnostic item verbs: locate an
//! item anywhere in the ledger; mutate metadata, with `--status`
//! going through the transition machine only.

use myco_core::{is_valid_label, Actor, EdgeKind, Item, ItemId, Priority, Status};
use myco_store::Store;
use serde_json::json;

use crate::items;
use crate::ops;
use crate::registry::{Args, Ctx};

/// The set-verb flag bundle — applied to an `Item` after the
/// status transition check.
#[derive(Default, Clone)]
pub(crate) struct SetFlags {
    pub(crate) status: Option<Status>,
    /// `--steal` — take an item over from a different owner
    /// (claim transitions and owner reassignment on work-state
    /// items are guarded; see [`ownership_guard`]).
    pub(crate) steal: bool,
    pub(crate) pri: Option<Priority>,
    pub(crate) owner: Option<Actor>,
    pub(crate) label: Vec<String>,
    pub(crate) unlabel: Vec<String>,
    pub(crate) blocks: Vec<String>,
    pub(crate) unblock: Vec<ItemId>,
    pub(crate) links: Vec<String>,
    pub(crate) unlink: Vec<ItemId>,
    pub(crate) epic: Option<String>,
    pub(crate) milestone: Option<String>,
    /// Raw `--due` value — kept stringly so `""` clears and the
    /// date validates at apply time (like the group names).
    pub(crate) due: Option<String>,
    /// Raw `--est` value — `""` clears, else a points count.
    pub(crate) est: Option<String>,
    pub(crate) cycle: Option<String>,
}

/// The decoded `item set` flags — shared by the CLI handler and
/// the wire handler (`myco serve`'s `item.set`).
pub(crate) fn set_flags(a: &Args) -> Result<SetFlags, String> {
    Ok(SetFlags {
        status: a.parsed("status")?,
        steal: a.flag("steal"),
        pri: a.parsed("pri")?,
        owner: a.parsed("owner")?,
        label: a.many("label"),
        unlabel: a.many("unlabel"),
        blocks: a.many("blocks"),
        unblock: a.many_parsed("unblock")?,
        links: a.many("links"),
        unlink: a.many_parsed("unlink")?,
        epic: a.opt("epic").map(str::to_string),
        milestone: a.opt("milestone").map(str::to_string),
        due: a.opt("due").map(str::to_string),
        est: a.opt("est").map(str::to_string),
        cycle: a.opt("cycle").map(str::to_string),
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
            && self.links.is_empty()
            && self.unlink.is_empty()
            && self.epic.is_none()
            && self.milestone.is_none()
            && self.due.is_none()
            && self.est.is_none()
            && self.cycle.is_none()
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

/// The ownership guard — assigning an item whose owner is someone
/// else is refused unless `--steal` is passed. It fires on claim
/// transitions (`→ claimed`) and on owner reassignment while the
/// item sits in a work state (`claimed`/`in-progress`); open-item
/// triage stays free. `me` is the resolved actor
/// ([`ops::resolve_actor`] — `--owner` already folded in).
fn ownership_guard(item: &Item, flags: &SetFlags, me: &Actor) -> Result<(), String> {
    if flags.steal || flags.owner.is_none() {
        return Ok(());
    }
    let Some(existing) = &item.owner else {
        return Ok(());
    };
    if *existing == *me {
        return Ok(());
    }
    let claim_transition = flags.status == Some(Status::Claimed);
    let work_state = matches!(item.status, Status::Claimed | Status::InProgress);
    if claim_transition || work_state {
        return Err(format!(
            "{}: owned by {existing} — `--steal` takes it over",
            item.id
        ));
    }
    Ok(())
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
    for raw in &flags.links {
        let (add, s) = signed(raw);
        let id: ItemId = s
            .parse()
            .map_err(|e: myco_core::ParseItemIdError| e.to_string())?;
        if add {
            item.links.insert(id);
        } else {
            item.links.remove(&id);
        }
    }
    for id in &flags.unlink {
        item.links.remove(id);
    }
    for (what, flag, slot) in [
        ("epic", &flags.epic, &mut item.epic),
        ("milestone", &flags.milestone, &mut item.milestone),
        ("cycle", &flags.cycle, &mut item.cycle),
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
    if let Some(d) = &flags.due {
        if d.is_empty() {
            item.due = None; // `--due ""` clears
        } else {
            item.due = Some(
                myco_core::date::parse(d).map_err(|e| format!("invalid due date {d:?} — {e}"))?,
            );
        }
    }
    if let Some(e) = &flags.est {
        if e.is_empty() {
            item.estimate = None; // `--est ""` clears
        } else {
            let n: u32 = e
                .parse()
                .map_err(|_| format!("invalid est value {e:?} — expected a point count"))?;
            if n > myco_core::MAX_ESTIMATE {
                return Err(format!(
                    "estimate {n} exceeds max {}",
                    myco_core::MAX_ESTIMATE
                ));
            }
            item.estimate = Some(n);
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

/// What [`set_item`] produced — the post-mutation item, the
/// location label, the session claim entry when one was written,
/// and the previous owner when `--steal` took the item over.
pub(crate) struct SetOutcome {
    /// The item after mutation (`None` when `done` removed a task
    /// line).
    pub item: Option<Item>,
    /// Ledger location label for reporting.
    pub location: String,
    /// The `claimed` handoff entry `--session` wrote, if any.
    pub claim: Option<myco_core::HandoffEntry>,
    /// The owner `--steal` took the item from, if a takeover
    /// happened — reported so callers can surface it.
    pub taken_from: Option<Actor>,
}

/// `item set` + an optional `--session` claim tag — the shared
/// mutation path when absent, [`crate::continuity::tag_claim`]
/// (mutation + `claimed` entry, atomically) when present.
/// `me` is the resolved actor ([`ops::resolve_actor`] with the
/// verb's explicit actor already folded in). A `→ claimed`
/// transition with no `--owner` assigns `me`; a `--steal`
/// takeover appends a `taken over from <actor>` handoff entry —
/// both ride the caller's one commit.
pub(crate) fn set_item(
    store: &impl Store,
    id: ItemId,
    flags: &SetFlags,
    session: Option<String>,
    me: &Actor,
) -> Result<SetOutcome, String> {
    let mut flags = flags.clone();
    // →claimed assigns the resolved actor when no owner named —
    // the claim always names its claimer
    if flags.status == Some(Status::Claimed) && flags.owner.is_none() {
        flags.owner = Some(me.clone());
    }
    // snapshot for takeover detection — decidable up front: the
    // post-mutation owner is `flags.owner` whenever one is set
    let prior_owner = items::find_item(store, id)?.and_then(|(i, _, _)| i.owner);
    let taken_from = if flags.steal && flags.owner.is_some() {
        prior_owner.filter(|p| flags.owner.as_ref() != Some(p))
    } else {
        None
    };
    let outcome = match session {
        Some(s) => {
            let actor = flags.owner.clone();
            let (item, location, entry) =
                crate::continuity::tag_claim(store, id, &flags, actor, &s, me)?;
            SetOutcome {
                item,
                location,
                claim: Some(entry),
                taken_from: None, // filled below
            }
        }
        None => {
            let (item, location) = mutate_item(store, id, &flags, me)?;
            SetOutcome {
                item,
                location,
                claim: None,
                taken_from: None, // filled below
            }
        }
    };
    // the takeover audit entry — same commit as the mutation
    // (the caller's autocommit sweeps both). Skipped when the
    // mutation removed the item entirely (`done` on a task) —
    // there's nothing live left to hang the note on.
    if let (Some(from), Some(_)) = (&taken_from, &outcome.item) {
        let entry = myco_core::HandoffEntry {
            date: items::today(),
            actor: me.clone(),
            session: None,
            note: format!("taken over from {from}"),
        };
        store
            .append_handoff(id, &entry)
            .map_err(|e| e.to_string())?;
    }
    Ok(SetOutcome {
        taken_from,
        ..outcome
    })
}

/// The `item set` verb.
pub fn h_set(ctx: &Ctx, a: &Args) -> Result<i32, String> {
    let id = a.req::<ItemId>("id")?;
    let flags = set_flags(a)?;
    let session = a.opt("session").map(str::to_string);
    if flags.is_noop() && session.is_none() {
        return Err("item set: nothing to change — pass at least one flag".into());
    }
    // the terminals are one-way — the `item set` worth confirming
    if let Some(to @ (Status::Done | Status::Cancelled)) = flags.status {
        if crate::confirm::needed(a.flag("yes"))
            && !crate::confirm::ask(&format!("set {id} → {to}? {to} is terminal."))
        {
            eprintln!("myco: cancelled");
            return Ok(1);
        }
    }
    let (proj, store) = items::open(ctx.dir.as_deref(), ctx.root.as_deref())?;
    // `--owner` doubles as the actor here (the resolution chain's
    // first rung) — the guard and the commit trailer agree
    let me = ops::resolve_actor(Some(&proj.worktree), flags.owner.as_ref());
    let outcome = set_item(&store, id, &flags, session, &me)?;
    let committed = items::commit(
        &proj.worktree,
        &format!("chore: set {id}"),
        Some(&me),
        ctx.no_commit,
    )?;
    let human = outcome
        .item
        .as_ref()
        .map(|i| format!("set {}", items::compact(i)))
        .unwrap_or_else(|| {
            format!(
                "set {id} {} — line removed",
                flags
                    .status
                    .map_or("done".to_string(), |s| s.token().to_string())
            )
        });
    let mut payload = json!({"item": outcome.item, "location": outcome.location});
    if let Some(entry) = outcome.claim {
        payload["claim"] = json!(entry);
    }
    if let Some(from) = outcome.taken_from {
        payload["taken_from"] = json!(from);
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
/// `me` is the resolved actor — the ownership guard compares the
/// item's owner against it. Returns the post-mutation item
/// (`None` when done removed a task line) and a location label
/// for reporting.
pub(crate) fn mutate_item(
    store: &impl Store,
    id: ItemId,
    flags: &SetFlags,
    me: &Actor,
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
        ownership_guard(target, flags, me)?;
        apply_set(target, flags)?;
        if status_change {
            let Some(current) = ledger.item(&loc) else {
                return Err(format!("{id}: no such task"));
            };
            match current.status {
                Status::Done | Status::Cancelled => {
                    let terminal = current.status;
                    let mut item = ledger
                        .take(&loc)
                        .ok_or_else(|| format!("{id}: no such task"))?;
                    // DONE.md holds the terminal pair — archive
                    // the whole subtree under the status it
                    // reached (done drops its token, cancelled
                    // keeps `status:cancelled`)
                    items::walk_descendants_to(&mut item, terminal)?;
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
        ownership_guard(&file.meta, flags, me)?;
        apply_set(&mut file.meta, flags)?;
        let item = file.meta.clone();
        store.save_item(&file).map_err(|e| e.to_string())?;
        let location = items::item_rel_path(store, &item)?;
        Ok((Some(item), location))
    }
}
