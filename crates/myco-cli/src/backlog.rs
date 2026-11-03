//! `myco backlog add|promote|list` — deferred items in BACKLOG.md.

use myco_core::{Item, ItemId, ItemKind, Status};
use myco_store::Store;
use serde_json::json;

use crate::items;
use crate::registry::{Args, Ctx};

/// The `backlog add` mutation — allocs a T-id straight into
/// BACKLOG.md (backlog items are born deferred). Caller commits.
pub fn add_item(
    store: &impl Store,
    title: &str,
    section: Option<&str>,
    meta: &myco_core::NewItemMeta,
) -> Result<Item, String> {
    let id = store.alloc_id(ItemKind::Task).map_err(|e| e.to_string())?;
    // created deferred — backlog items are born parked
    let mut item = Item::new(id, title)
        .status(Status::Deferred)
        .created(items::today());
    meta.apply(&mut item)?;
    let mut ledger = items::load_ledger(store)?;
    ledger.put_backlog(item.clone(), section);
    items::save_ledger(store, &ledger)?;
    Ok(item)
}

/// The `backlog promote` mutation — `deferred → open`, moves the
/// line (subtree included) into TODO.md. Caller commits.
pub fn promote_item(store: &impl Store, id: ItemId) -> Result<Item, String> {
    items::require_kind(id, ItemKind::Task, "backlog promote")?;
    let mut ledger = items::load_ledger(store)?;
    let loc = ledger
        .locate(id)
        .ok_or_else(|| format!("{id}: no such task"))?;
    if !loc.backlog {
        return Err(format!("{id} isn't in BACKLOG.md — nothing to promote"));
    }
    let mut item = ledger
        .take(&loc)
        .ok_or_else(|| format!("{id}: no such task"))?;
    items::walk_to(&mut item, Status::Open)?;
    // the subtree unparks with its parent — deferred
    // children come back to open (TODO.md holds no
    // deferred items)
    items::walk_descendants_to(&mut item, Status::Open)?;
    ledger.put_todo(item.clone(), None);
    items::save_ledger(store, &ledger)?;
    Ok(item)
}

/// The `backlog add` verb.
pub fn h_add(ctx: &Ctx, a: &Args) -> Result<i32, String> {
    let (proj, store) = items::open(ctx.dir.as_deref(), ctx.root.as_deref())?;
    let item = add_item(
        &store,
        a.text("title")?,
        a.opt("section"),
        &items::meta_args(a)?,
    )?;
    let committed = items::commit(
        &proj.worktree,
        &format!("chore: add {}", item.id),
        ctx.no_commit,
    )?;
    items::report(
        json!({"item": item, "file": "BACKLOG.md"}),
        committed,
        ctx.json,
        &format!("added {}", items::compact(&item)),
    )
}

/// The `backlog promote` verb.
pub fn h_promote(ctx: &Ctx, a: &Args) -> Result<i32, String> {
    let (proj, store) = items::open(ctx.dir.as_deref(), ctx.root.as_deref())?;
    let id = a.req::<ItemId>("id")?;
    let item = promote_item(&store, id)?;
    let committed = items::commit(
        &proj.worktree,
        &format!("chore: promote {id}"),
        ctx.no_commit,
    )?;
    items::report(
        json!({"item": item, "file": "TODO.md"}),
        committed,
        ctx.json,
        &format!("promoted {id} → TODO.md"),
    )
}

/// The `backlog list` verb.
pub fn h_list(ctx: &Ctx, a: &Args) -> Result<i32, String> {
    let (_proj, store) = items::open_read(ctx.dir.as_deref(), ctx.root.as_deref())?;
    let list = items::list_args(a)?;
    let all = crate::todo::doc_items(&store, true)?;
    let found: Vec<(usize, &Item)> = items::tree_view(&all)
        .into_iter()
        .filter(|(_, i)| list.matches(i))
        .take(list.take())
        .collect();
    items::print_tree(&found, ctx.json)
}
