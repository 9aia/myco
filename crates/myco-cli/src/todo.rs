//! `myco todo add|done|defer|list` — task items in TODO.md.

use myco_core::{Item, ItemId, ItemKind, Status};
use myco_store::Store;
use serde_json::json;

use crate::items;
use crate::registry::{Args, Ctx};

/// The `todo add` mutation — allocs a T-id and appends to
/// TODO.md (or nests under `--parent`; a backlog parent's child
/// parks with it). The parent is validated before the id is
/// allocated so failed adds don't burn ids. Returns the new item
/// plus the doc it landed in. Caller commits.
pub fn add_item(
    store: &impl Store,
    title: &str,
    section: Option<&str>,
    parent: Option<ItemId>,
    meta: &myco_core::NewItemMeta,
) -> Result<(Item, &'static str), String> {
    let mut ledger = items::load_ledger(store)?;
    // validate the parent before allocating — failed adds
    // must not burn ids
    let ploc = match parent {
        Some(pid) => {
            items::require_kind(pid, ItemKind::Task, "todo add --parent")?;
            Some(
                ledger
                    .locate(pid)
                    .ok_or_else(|| format!("{pid}: no such task — can't nest under it"))?,
            )
        }
        None => None,
    };
    let id = store.alloc_id(ItemKind::Task).map_err(|e| e.to_string())?;
    let mut item = Item::new(id, title).created(items::today());
    meta.apply(&mut item)?;
    let file = match ploc {
        Some(loc) => {
            if loc.backlog {
                // a backlog parent's children park with it
                item.status = Status::Deferred;
            }
            match ledger.item_mut(&loc) {
                Some(parent) => parent.children.push(item.clone()),
                None => {
                    return Err(format!(
                        "{}: stale task location",
                        parent.map_or_else(|| "<parent>".into(), |p| p.to_string())
                    ))
                }
            }
            if loc.backlog {
                "BACKLOG.md"
            } else {
                "TODO.md"
            }
        }
        None => {
            ledger.put_todo(item.clone(), section);
            "TODO.md"
        }
    };
    items::save_ledger(store, &ledger)?;
    Ok((item, file))
}

/// The `todo done` mutation — walks to `done`, removes the line
/// (subtree included), and archives the subtree into `DONE.md`.
/// Caller commits. Returns the removed item.
pub fn done_item(store: &impl Store, id: ItemId) -> Result<Item, String> {
    items::require_kind(id, ItemKind::Task, "todo done")?;
    let mut ledger = items::load_ledger(store)?;
    let loc = ledger
        .locate(id)
        .ok_or_else(|| format!("{id}: no such task"))?;
    if loc.backlog {
        return Err(format!(
            "{id} is deferred (BACKLOG.md) — `myco backlog promote` it first"
        ));
    }
    // load + stage the archive before any write — a parse error
    // or duplicate id fails the verb with the ledger untouched
    let mut done = items::load_done(store)?;
    // take removes the whole subtree — children die with
    // their parent; done on a child removes just the child
    let mut item = ledger
        .take(&loc)
        .ok_or_else(|| format!("{id}: no such task"))?;
    items::walk_to(&mut item, Status::Done)?;
    // DONE.md emit is done-only — the whole subtree archives done
    items::walk_descendants_to(&mut item, Status::Done)?;
    items::done_push(&mut done, item.clone())?;
    items::save_ledger(store, &ledger)?;
    items::save_done(store, &done)?;
    Ok(item)
}

/// The `todo cancel` mutation — walks to `cancelled`, removes the
/// line (subtree included), and archives the subtree into
/// `DONE.md` with the cancelled status preserved (unlike `done`,
/// the archive records *why* it left). Works on BACKLOG.md items
/// too — cancelling parked work shouldn't require promoting it
/// first. Caller commits. Returns the removed item.
pub fn cancel_item(store: &impl Store, id: ItemId) -> Result<Item, String> {
    items::require_kind(id, ItemKind::Task, "todo cancel")?;
    let mut ledger = items::load_ledger(store)?;
    let loc = ledger
        .locate(id)
        .ok_or_else(|| format!("{id}: no such task"))?;
    // load + stage the archive before any write — a parse error
    // or duplicate id fails the verb with the ledger untouched
    let mut done = items::load_done(store)?;
    let mut item = ledger
        .take(&loc)
        .ok_or_else(|| format!("{id}: no such task"))?;
    items::walk_to(&mut item, Status::Cancelled)?;
    // DONE.md holds the terminal pair — the whole subtree
    // archives cancelled, like `todo done` archives done
    items::walk_descendants_to(&mut item, Status::Cancelled)?;
    items::done_push(&mut done, item.clone())?;
    items::save_ledger(store, &ledger)?;
    items::save_done(store, &done)?;
    Ok(item)
}

/// The `todo defer` mutation — walks to `deferred`, moves the
/// line (subtree included) into BACKLOG.md. Caller commits.
pub fn defer_item(store: &impl Store, id: ItemId) -> Result<Item, String> {
    items::require_kind(id, ItemKind::Task, "todo defer")?;
    let mut ledger = items::load_ledger(store)?;
    let loc = ledger
        .locate(id)
        .ok_or_else(|| format!("{id}: no such task"))?;
    if loc.backlog {
        return Err(format!("{id} is already deferred (BACKLOG.md)"));
    }
    let mut item = ledger
        .take(&loc)
        .ok_or_else(|| format!("{id}: no such task"))?;
    items::walk_to(&mut item, Status::Deferred)?;
    // the whole subtree parks too — BACKLOG.md is
    // deferred-only
    items::walk_descendants_to(&mut item, Status::Deferred)?;
    ledger.put_backlog(item.clone(), None);
    items::save_ledger(store, &ledger)?;
    Ok(item)
}

/// The `todo list`/`backlog list` read — filtered items in tree
/// order, depth flattened away (`myco serve` returns it raw).
pub fn list_items(
    store: &impl Store,
    backlog: bool,
    filter: &items::Filter,
) -> Result<Vec<Item>, String> {
    let all = doc_items(store, backlog)?;
    Ok(items::tree_view(&all)
        .into_iter()
        .filter(|(_, i)| filter.matches(i))
        .take(filter.take())
        .map(|(_, i)| i.clone())
        .collect())
}

/// The `todo add` verb.
pub fn h_add(ctx: &Ctx, a: &Args) -> Result<i32, String> {
    let (proj, store) = items::open(ctx.dir.as_deref(), ctx.root.as_deref())?;
    let parent: Option<ItemId> = a.parsed("parent")?;
    let meta = items::meta_args(a)?;
    let (item, file) = add_item(&store, a.text("title")?, a.opt("section"), parent, &meta)?;
    let committed = items::commit(
        &proj.worktree,
        &format!("chore: add {}", item.id),
        meta.owner.as_ref(),
        ctx.no_commit,
    )?;
    items::report(
        json!({"item": item, "file": file, "parent": parent.map(|p| p.to_string())}),
        committed,
        ctx.json,
        &format!("added {}", items::compact(&item)),
    )
}

/// The `todo done` verb.
pub fn h_done(ctx: &Ctx, a: &Args) -> Result<i32, String> {
    let id = a.req::<ItemId>("id")?;
    if crate::confirm::needed(a.flag("yes"))
        && !crate::confirm::ask(&format!(
            "done {id} — archives it + its subtree into DONE.md?"
        ))
    {
        eprintln!("myco: cancelled");
        return Ok(1);
    }
    let (proj, store) = items::open(ctx.dir.as_deref(), ctx.root.as_deref())?;
    let item = done_item(&store, id)?;
    let committed = items::commit(
        &proj.worktree,
        &format!("chore: done {id}"),
        None,
        ctx.no_commit,
    )?;
    items::report(
        json!({"item": item}),
        committed,
        ctx.json,
        &format!("done {id} — {}", item.title),
    )
}

/// The `todo cancel` verb.
pub fn h_cancel(ctx: &Ctx, a: &Args) -> Result<i32, String> {
    let id = a.req::<ItemId>("id")?;
    if crate::confirm::needed(a.flag("yes"))
        && !crate::confirm::ask(&format!(
            "cancel {id} — archives it + its subtree into DONE.md as cancelled?"
        ))
    {
        eprintln!("myco: cancelled");
        return Ok(1);
    }
    let (proj, store) = items::open(ctx.dir.as_deref(), ctx.root.as_deref())?;
    let item = cancel_item(&store, id)?;
    let committed = items::commit(
        &proj.worktree,
        &format!("chore: cancel {id}"),
        None,
        ctx.no_commit,
    )?;
    items::report(
        json!({"item": item, "file": "DONE.md"}),
        committed,
        ctx.json,
        &format!("cancelled {id} — {}", item.title),
    )
}

/// The `todo defer` verb.
pub fn h_defer(ctx: &Ctx, a: &Args) -> Result<i32, String> {
    let (proj, store) = items::open(ctx.dir.as_deref(), ctx.root.as_deref())?;
    let id = a.req::<ItemId>("id")?;
    let item = defer_item(&store, id)?;
    let committed = items::commit(
        &proj.worktree,
        &format!("chore: defer {id}"),
        None,
        ctx.no_commit,
    )?;
    items::report(
        json!({"item": item, "file": "BACKLOG.md"}),
        committed,
        ctx.json,
        &format!("deferred {id} → BACKLOG.md"),
    )
}

/// The `todo list` verb.
pub fn h_list(ctx: &Ctx, a: &Args) -> Result<i32, String> {
    let (_proj, store) = items::open_read(ctx.dir.as_deref(), ctx.root.as_deref())?;
    let list = items::list_args(a)?;
    let all = doc_items(&store, false)?;
    let found: Vec<(usize, &Item)> = items::tree_view(&all)
        .into_iter()
        .filter(|(_, i)| list.matches(i))
        .take(list.take())
        .collect();
    items::print_tree(&found, ctx.json)
}

/// All items in one doc, cloned out (shared by the list verbs).
pub fn doc_items(store: &impl Store, backlog: bool) -> Result<Vec<Item>, String> {
    let doc_items = if backlog {
        store.backlog().map(|d| d.items().cloned().collect())
    } else {
        store.todo().map(|d| d.items().cloned().collect())
    };
    doc_items.map_err(|e| e.to_string())
}
