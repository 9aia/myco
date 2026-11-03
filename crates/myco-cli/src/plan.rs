//! `myco plan new|list` — execution plans in
//! `plans/pl-NNNN-<slug>.md` with a phase checklist.

use myco_core::ItemKind;

use crate::items;
use crate::registry::{Args, Ctx};
use crate::templates;

/// The `plan new` verb.
pub fn h_new(ctx: &Ctx, a: &Args) -> Result<i32, String> {
    let (proj, store) = items::open(ctx.dir.as_deref(), ctx.root.as_deref())?;
    items::new_file(
        &proj,
        &store,
        &items::NewItem {
            kind: ItemKind::Plan,
            title: a.text("title")?.to_string(),
            body: items::body_args(a)?.as_deref(),
            meta: &items::meta_args(a)?,
            template: templates::plan,
            word: "plan",
            json_out: ctx.json,
            no_commit: ctx.no_commit,
        },
    )
}

/// The `plan list` verb.
pub fn h_list(ctx: &Ctx, a: &Args) -> Result<i32, String> {
    let (_proj, store) = items::open_read(ctx.dir.as_deref(), ctx.root.as_deref())?;
    items::list_files(
        &store,
        ItemKind::Plan,
        false,
        &items::list_args(a)?,
        ctx.json,
    )
}
