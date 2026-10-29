//! `myco todo add|done|defer|list` — task items in TODO.md.

use std::path::PathBuf;

use clap::Subcommand;
use myco_core::{Item, ItemId, ItemKind, Status};
use myco_store::Store;
use serde_json::json;

use crate::items;

#[derive(Subcommand)]
pub enum Cmd {
    /// Add a task — allocs a T-id, appends to a section of TODO.md.
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
    /// Mark done — walks the state machine to `done`, then removes
    /// the line (git history keeps the record).
    Done {
        /// Task id (`T-NNN`).
        id: ItemId,
    },
    /// Park a task — transition to `deferred`, move the line to
    /// BACKLOG.md.
    Defer {
        /// Task id (`T-NNN`).
        id: ItemId,
    },
    /// List tasks.
    List {
        #[command(flatten)]
        list: items::ListFlags,
    },
}

/// The `todo` verb group.
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
            let mut item = Item::new(id, title).created(items::today());
            meta.apply(&mut item)?;
            let mut ledger = items::Ledger::load(&store)?;
            ledger.put_todo(item.clone(), section.as_deref());
            ledger.save(&store)?;
            let committed = items::commit(&proj.worktree, &format!("chore: add {id}"), no_commit)?;
            items::report(
                json!({"item": item, "file": "TODO.md"}),
                committed,
                json_out,
                &format!("added {}", items::compact(&item)),
            )
        }
        Cmd::Done { id } => {
            items::require_kind(id, ItemKind::Task, "todo done")?;
            let mut ledger = items::Ledger::load(&store)?;
            let loc = ledger
                .locate(id)
                .ok_or_else(|| format!("{id}: no such task"))?;
            if loc.backlog {
                return Err(format!(
                    "{id} is deferred (BACKLOG.md) — `myco backlog promote` it first"
                ));
            }
            let mut item = ledger.take(loc);
            items::walk_to(&mut item, Status::Done)?;
            ledger.save(&store)?;
            let committed = items::commit(&proj.worktree, &format!("chore: done {id}"), no_commit)?;
            items::report(
                json!({"item": item}),
                committed,
                json_out,
                &format!("done {id} — {}", item.title),
            )
        }
        Cmd::Defer { id } => {
            items::require_kind(id, ItemKind::Task, "todo defer")?;
            let mut ledger = items::Ledger::load(&store)?;
            let loc = ledger
                .locate(id)
                .ok_or_else(|| format!("{id}: no such task"))?;
            if loc.backlog {
                return Err(format!("{id} is already deferred (BACKLOG.md)"));
            }
            let mut item = ledger.take(loc);
            items::walk_to(&mut item, Status::Deferred)?;
            ledger.put_backlog(item.clone(), None);
            ledger.save(&store)?;
            let committed =
                items::commit(&proj.worktree, &format!("chore: defer {id}"), no_commit)?;
            items::report(
                json!({"item": item, "file": "BACKLOG.md"}),
                committed,
                json_out,
                &format!("deferred {id} → BACKLOG.md"),
            )
        }
        Cmd::List { list } => {
            let found: Vec<Item> = doc_items(&store, false)?
                .into_iter()
                .filter(|i| list.matches(i))
                .collect();
            items::print_list(&found, json_out)
        }
    }
}

/// All items in one doc, cloned out (shared by the list verbs).
pub fn doc_items(store: &myco_store_md::MdStore, backlog: bool) -> Result<Vec<Item>, String> {
    let doc_items = if backlog {
        store.backlog().map(|d| d.items().cloned().collect())
    } else {
        store.todo().map(|d| d.items().cloned().collect())
    };
    doc_items.map_err(|e| e.to_string())
}
