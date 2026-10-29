//! `myco backlog add|promote|list` — deferred items in BACKLOG.md.

use std::path::PathBuf;

use clap::Subcommand;
use myco_core::{Item, ItemId, ItemKind, Status};
use myco_store::Store;
use serde_json::json;

use crate::items;

#[derive(Subcommand)]
pub enum Cmd {
    /// Add a deferred item — allocs a T-id straight into BACKLOG.md.
    Add {
        /// One-line title.
        title: String,
        /// Section to add under (created when absent; default: the
        /// first section, or an unsectioned run).
        #[arg(long)]
        section: Option<String>,
        #[command(flatten)]
        meta: items::MetaFlags,
    },
    /// Re-activate a parked item — `deferred → open`, move the line
    /// to TODO.md.
    Promote {
        /// Task id (`T-NNN`).
        id: ItemId,
    },
    /// List backlog items.
    List {
        #[command(flatten)]
        list: items::ListFlags,
    },
}

/// The `backlog` verb group.
pub fn run(
    cmd: Cmd,
    dir: Option<PathBuf>,
    root: Option<PathBuf>,
    json_out: bool,
    no_commit: bool,
) -> Result<i32, String> {
    let (proj, store) = items::open(dir.as_deref(), root.as_deref())?;
    match cmd {
        Cmd::Add {
            title,
            section,
            meta,
        } => {
            let id = store.alloc_id(ItemKind::Task).map_err(|e| e.to_string())?;
            // created deferred — backlog items are born parked
            let mut item = Item::new(id, title)
                .status(Status::Deferred)
                .created(items::today());
            meta.apply(&mut item)?;
            let mut ledger = items::Ledger::load(&store)?;
            ledger.put_backlog(item.clone(), section.as_deref());
            ledger.save(&store)?;
            let committed = items::commit(&proj.worktree, &format!("chore: add {id}"), no_commit)?;
            items::report(
                json!({"item": item, "file": "BACKLOG.md"}),
                committed,
                json_out,
                &format!("added {}", items::compact(&item)),
            )
        }
        Cmd::Promote { id } => {
            items::require_kind(id, ItemKind::Task, "backlog promote")?;
            let mut ledger = items::Ledger::load(&store)?;
            let loc = ledger
                .locate(id)
                .ok_or_else(|| format!("{id}: no such task"))?;
            if !loc.backlog {
                return Err(format!("{id} isn't in BACKLOG.md — nothing to promote"));
            }
            let mut item = ledger.take(loc);
            items::walk_to(&mut item, Status::Open)?;
            ledger.put_todo(item.clone(), None);
            ledger.save(&store)?;
            let committed =
                items::commit(&proj.worktree, &format!("chore: promote {id}"), no_commit)?;
            items::report(
                json!({"item": item, "file": "TODO.md"}),
                committed,
                json_out,
                &format!("promoted {id} → TODO.md"),
            )
        }
        Cmd::List { list } => {
            let found: Vec<Item> = crate::todo::doc_items(&store, true)?
                .into_iter()
                .filter(|i| list.matches(i))
                .collect();
            items::print_list(&found, json_out)
        }
    }
}
