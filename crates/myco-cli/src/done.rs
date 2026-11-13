//! `myco done list|remove` — the DONE.md archive read + delete.
//! Archived tasks (done and cancelled) leave TODO.md but stay
//! here, filterable like every other list; `remove` drops a
//! subtree from the archive entirely — for scaffold junk and
//! mistakes, since reopen (`item set --status open`) is the
//! verb for work that isn't actually finished.

use myco_core::Item;
use myco_store::Store;
use serde_json::json;

use crate::items;
use crate::registry::{Args, Ctx};

/// All archived items, cloned out (shared by the verb + wire).
pub fn done_items(store: &impl Store) -> Result<Vec<Item>, String> {
    store
        .done()
        .map(|d| d.items().cloned().collect())
        .map_err(|e| e.to_string())
}

/// The `done list` verb.
pub fn h_list(ctx: &Ctx, a: &Args) -> Result<i32, String> {
    let (_proj, store) = items::open_read(ctx.dir.as_deref(), ctx.root.as_deref())?;
    let list = items::list_args(a)?;
    let all = done_items(&store)?;
    let found: Vec<(usize, &Item)> = items::tree_view(&all)
        .into_iter()
        .filter(|(_, i)| list.matches(i))
        .take(list.take())
        .collect();
    items::print_tree(&found, ctx.json)
}

/// The `done remove` mutation — delete an archived task subtree
/// from DONE.md outright (the ledger forgets it; a `handoffs/`
/// file, if any, stays). Caller commits. Returns the removed
/// item.
pub fn remove_item(store: &impl Store, id: myco_core::ItemId) -> Result<Item, String> {
    items::require_kind(id, myco_core::ItemKind::Task, "done remove")?;
    let mut done = items::load_done(store)?;
    let Some((section, path)) = done.locate(id) else {
        return Err(format!("{id}: no such archived task"));
    };
    let item = done
        .take(section, &path)
        .ok_or_else(|| format!("{id}: stale DONE.md location"))?;
    items::save_done(store, &done)?;
    Ok(item)
}

/// The `done remove` verb.
pub fn h_remove(ctx: &Ctx, a: &Args) -> Result<i32, String> {
    let id = a.req::<myco_core::ItemId>("id")?;
    if crate::confirm::needed(a.flag("yes"))
        && !crate::confirm::ask(&format!(
            "remove {id} — deletes it + its subtree from DONE.md for good?"
        ))
    {
        eprintln!("myco: cancelled");
        return Ok(1);
    }
    let (proj, store) = items::open(ctx.dir.as_deref(), ctx.root.as_deref())?;
    let item = remove_item(&store, id)?;
    let committed = items::commit(
        &proj.worktree,
        &format!("chore: remove {id}"),
        None,
        ctx.no_commit,
    )?;
    items::report(
        json!({"item": item, "file": "DONE.md"}),
        committed,
        ctx.json,
        &format!("removed {id} from DONE.md — {}", item.title),
    )
}
