//! `myco done list` — the DONE.md archive read. Done tasks leave
//! TODO.md but stay here, filterable like every other list.

use myco_core::Item;
use myco_store::Store;

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
