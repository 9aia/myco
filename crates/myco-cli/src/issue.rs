//! `myco issue new|list|resolve|archive|comment` — one md file
//! per issue under `issues/` (archived files in `issues/archive/`).

use myco_core::{ItemId, ItemKind, Status};
use myco_store::Store;
use serde_json::json;

use crate::items;
use crate::registry::{Args, Ctx};
use crate::templates;

/// The `issue new` verb.
pub fn h_new(ctx: &Ctx, a: &Args) -> Result<i32, String> {
    let (proj, store) = items::open(ctx.dir.as_deref(), ctx.root.as_deref())?;
    items::new_file(
        &proj,
        &store,
        &items::NewItem {
            kind: ItemKind::Issue,
            title: a.text("title")?.to_string(),
            body: body_source(a)?.as_deref(),
            meta: &items::meta_args(a)?,
            template: templates::issue,
            word: "issue",
            json_out: ctx.json,
            no_commit: ctx.no_commit,
        },
    )
}

/// `--body/--text` — shared body source for the `new` verbs.
fn body_source(a: &Args) -> Result<Option<String>, String> {
    items::body_args(a)
}

/// The `issue list` verb.
pub fn h_list(ctx: &Ctx, a: &Args) -> Result<i32, String> {
    let (_proj, store) = items::open_read(ctx.dir.as_deref(), ctx.root.as_deref())?;
    items::list_files(
        &store,
        ItemKind::Issue,
        a.flag("archived"),
        &items::list_args(a)?,
        ctx.json,
    )
}

/// The `issue resolve` verb.
pub fn h_resolve(ctx: &Ctx, a: &Args) -> Result<i32, String> {
    let (proj, store) = items::open(ctx.dir.as_deref(), ctx.root.as_deref())?;
    let id = a.req::<ItemId>("id")?;
    items::require_kind(id, ItemKind::Issue, "issue resolve")?;
    let mut file = items::item_for_edit(&store, id)?;
    items::walk_to(&mut file.meta, Status::Done)?;
    if let Some(res) = a.opt("resolution") {
        templates::append_to_section(&mut file.body, "## Resolution", res);
        file.body = myco_core::canonical_body(&file.body).to_string();
    }
    let item = file.meta.clone();
    store.save_item(&file).map_err(|e| e.to_string())?;
    let committed = items::commit(
        &proj.worktree,
        &format!("chore: resolve {id}"),
        ctx.no_commit,
    )?;
    items::report(
        json!({"item": item}),
        committed,
        ctx.json,
        &format!("resolved {id}"),
    )
}

/// The `issue archive` verb.
pub fn h_archive(ctx: &Ctx, a: &Args) -> Result<i32, String> {
    let (proj, store) = items::open(ctx.dir.as_deref(), ctx.root.as_deref())?;
    let id = a.req::<ItemId>("id")?;
    items::require_kind(id, ItemKind::Issue, "issue archive")?;
    let file = items::item_for_edit(&store, id)?;
    if file.meta.status != Status::Done {
        return Err(format!(
            "{id} must be done before archiving (status: {}) — resolve it first",
            file.meta.status
        ));
    }
    store.archive_item(id).map_err(|e| e.to_string())?;
    // asked after the move so the impl reports the
    // archive-scope path itself
    let rel = items::item_rel_path(&store, &file.meta)?;
    let committed = items::commit(
        &proj.worktree,
        &format!("chore: archive {id}"),
        ctx.no_commit,
    )?;
    items::report(
        json!({"item": file.meta, "archived_to": rel}),
        committed,
        ctx.json,
        &format!("archived {id} → {rel}"),
    )
}

/// The `issue comment` verb.
pub fn h_comment(ctx: &Ctx, a: &Args) -> Result<i32, String> {
    let (proj, store) = items::open(ctx.dir.as_deref(), ctx.root.as_deref())?;
    let id = a.req::<ItemId>("id")?;
    items::require_kind(id, ItemKind::Issue, "issue comment")?;
    let mut file = items::item_for_edit(&store, id)?;
    let line = format!(
        "- {} {}",
        myco_core::date::render(items::today()),
        a.text("text")?
    );
    templates::append_to_section(&mut file.body, "## Comments", &line);
    file.body = myco_core::canonical_body(&file.body).to_string();
    store.save_item(&file).map_err(|e| e.to_string())?;
    let committed = items::commit(
        &proj.worktree,
        &format!("chore: comment {id}"),
        ctx.no_commit,
    )?;
    items::report(
        json!({"item": file.meta}),
        committed,
        ctx.json,
        &format!("commented on {id}"),
    )
}
