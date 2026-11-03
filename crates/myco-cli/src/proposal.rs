//! `myco proposal new|list|status` — design proposals in
//! `proposals/proposal-NNNN-<slug>.md`.

use myco_core::{ItemId, ItemKind, Status};
use myco_store::Store;
use serde_json::json;

use crate::items;
use crate::registry::{Args, Ctx};
use crate::templates;

/// Proposal-domain words → machine [`Status`].
fn proposal_status(word: &str) -> Result<Status, String> {
    match word {
        "draft" => Ok(Status::Open),
        "accepted" => Ok(Status::Done),
        "rejected" => Ok(Status::Deferred),
        raw => raw.parse::<Status>().map_err(|_| {
            format!(
                "invalid proposal status {word:?} — expected draft|accepted|rejected \
                 (or a raw status: open|claimed|in-progress|blocked|done|deferred)"
            )
        }),
    }
}

/// The `proposal new` verb.
pub fn h_new(ctx: &Ctx, a: &Args) -> Result<i32, String> {
    let (proj, store) = items::open(ctx.dir.as_deref(), ctx.root.as_deref())?;
    items::new_file(
        &proj,
        &store,
        &items::NewItem {
            kind: ItemKind::Proposal,
            title: a.text("title")?.to_string(),
            body: items::body_args(a)?.as_deref(),
            meta: &items::meta_args(a)?,
            template: templates::proposal,
            word: "proposal",
            json_out: ctx.json,
            no_commit: ctx.no_commit,
        },
    )
}

/// The `proposal list` verb.
pub fn h_list(ctx: &Ctx, a: &Args) -> Result<i32, String> {
    let (_proj, store) = items::open_read(ctx.dir.as_deref(), ctx.root.as_deref())?;
    items::list_files(
        &store,
        ItemKind::Proposal,
        false,
        &items::list_args(a)?,
        ctx.json,
    )
}

/// The `proposal status` verb.
pub fn h_status(ctx: &Ctx, a: &Args) -> Result<i32, String> {
    let (proj, store) = items::open(ctx.dir.as_deref(), ctx.root.as_deref())?;
    let id = a.req::<ItemId>("id")?;
    items::require_kind(id, ItemKind::Proposal, "proposal status")?;
    let to = proposal_status(a.text("status")?)?;
    let mut file = items::item_for_edit(&store, id)?;
    let from = file.meta.status;
    items::walk_to(&mut file.meta, to)?;
    let item = file.meta.clone();
    store.save_item(&file).map_err(|e| e.to_string())?;
    let committed = items::commit(
        &proj.worktree,
        &format!("chore: proposal {id} → {to}"),
        ctx.no_commit,
    )?;
    items::report(
        json!({"item": item, "from": from.token(), "to": to.token()}),
        committed,
        ctx.json,
        &format!("{id}: {from} → {to}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proposal_status_mapping() {
        assert_eq!(proposal_status("draft"), Ok(Status::Open));
        assert_eq!(proposal_status("accepted"), Ok(Status::Done));
        assert_eq!(proposal_status("rejected"), Ok(Status::Deferred));
        assert_eq!(proposal_status("blocked"), Ok(Status::Blocked));
        assert!(proposal_status("wip").is_err());
    }
}
