//! `myco adr new|list|supersede` — architecture decision records
//! in `adrs/NNNN-<slug>.md` (Context / Decision / Consequences).

use myco_core::{ItemId, ItemKind, Status};
use myco_store::Store;
use serde_json::json;

use crate::items;
use crate::registry::{Args, Ctx};
use crate::templates;

/// The `adr new` verb.
pub fn h_new(ctx: &Ctx, a: &Args) -> Result<i32, String> {
    let (proj, store) = items::open(ctx.dir.as_deref(), ctx.root.as_deref())?;
    items::new_file(
        &proj,
        &store,
        &items::NewItem {
            kind: ItemKind::Adr,
            title: a.text("title")?.to_string(),
            body: items::body_args(a)?.as_deref(),
            meta: &items::meta_args(a)?,
            template: templates::adr,
            word: "adr",
            json_out: ctx.json,
            no_commit: ctx.no_commit,
        },
    )
}

/// The `adr list` verb.
pub fn h_list(ctx: &Ctx, a: &Args) -> Result<i32, String> {
    let (_proj, store) = items::open_read(ctx.dir.as_deref(), ctx.root.as_deref())?;
    items::list_files(
        &store,
        ItemKind::Adr,
        false,
        &items::list_args(a)?,
        ctx.json,
    )
}

/// The `adr supersede` verb.
pub fn h_supersede(ctx: &Ctx, a: &Args) -> Result<i32, String> {
    let (proj, store) = items::open(ctx.dir.as_deref(), ctx.root.as_deref())?;
    let id = a.req::<ItemId>("id")?;
    let by = a.req::<ItemId>("by")?;
    items::require_kind(id, ItemKind::Adr, "adr supersede")?;
    items::require_kind(by, ItemKind::Adr, "adr supersede --by")?;
    if id == by {
        return Err(format!("{id} can't supersede itself"));
    }
    let mut old = items::item_for_edit(&store, id)?;
    let mut new = items::item_for_edit(&store, by)?;
    items::walk_to(&mut old.meta, Status::Done)?;
    old.meta.superseded_by = Some(by);
    new.meta.supersedes = Some(id);
    store.save_item(&old).map_err(|e| e.to_string())?;
    store.save_item(&new).map_err(|e| e.to_string())?;
    let committed = items::commit(
        &proj.worktree,
        &format!("chore: supersede {id} by {by}"),
        None,
        ctx.no_commit,
    )?;
    items::report(
        json!({"superseded": old.meta, "by": new.meta}),
        committed,
        ctx.json,
        &format!("{id} superseded by {by}"),
    )
}
