//! `myco next` — the single highest-priority open item:
//! "what should I work on" (I-020, falls out of P-006). It's a
//! view eval with `limit = 1` — the filter flags are `view`'s
//! [`FilterFlags`] merged over an open tasks+issues default.
//! `--claim` is the only new surface: it mutates the emitted
//! item to `claimed` + an owner in one per-verb commit.

use std::path::PathBuf;

use clap::Args;
use myco_core::{eval, Actor, Item, SortKey, Status, ViewSpec};
use myco_store::Store;
use serde_json::json;

use crate::{continuity, digest, itemcmd, items, ops, view};

/// The `next` flag bundle — `view`'s filter flags plus `--sort`
/// (the engine's `pri,id` when unset) and `--claim`. No
/// `--limit`/`--group-by`/`--format`: the verb *is* limit 1
/// rendered as one compact line.
#[derive(Debug, Default, Args)]
pub struct NextFlags {
    #[command(flatten)]
    pub(crate) filter: view::FilterFlags,
    /// Sort keys — `pri,id,-created`; `-` flips a key's default
    /// direction. Default `pri,id`.
    #[arg(long, value_delimiter = ',')]
    sort: Vec<SortKey>,
    /// Claim the emitted item — status `claimed`, owner set to
    /// the `--owner` actor (else `cli`); `--owner` names the
    /// claimer here and doesn't filter. Mutating: one commit.
    #[arg(long)]
    pub(crate) claim: bool,
}

/// The default `next` spec — the builtin `next` view (open
/// tasks+issues, top item only), before config shadows/flags.
pub(crate) fn base_spec() -> ViewSpec {
    myco_ops::view::builtins()
        .into_iter()
        .find(|v| v.name == "next")
        .map(|v| v.spec)
        .unwrap_or_default()
}

/// Resolve the `next` named view — a repo's `[views.next]`
/// shadows the builtin like any other name — then merge flags,
/// validated. Under `--claim` the `--owner` flag names the
/// claimer, so it must not also restrict the pick to items that
/// owner already holds.
pub(crate) fn spec(root: &std::path::Path, flags: &NextFlags) -> Result<ViewSpec, String> {
    let (view, warnings) = myco_ops::view::resolve_lenient(root, "next", ops::CONFIG)?;
    view::warn(&warnings);
    let mut spec = view.spec;
    flags.filter.apply(&mut spec.filter);
    if flags.claim {
        spec.filter.owner = Vec::new();
    }
    if !flags.sort.is_empty() {
        spec.sort = flags.sort.clone();
    }
    spec.validate()?;
    Ok(spec)
}

/// The highest-priority live item under `spec` — the pick
/// behind the verb and `serve`'s `next` method. The universe is
/// top-level items only — children ride with their parent (the
/// `myco view` rule).
pub(crate) fn pick<'a>(scan: &'a digest::Scan, spec: &ViewSpec) -> Option<&'a Item> {
    let universe = scan.items.iter().filter(|i| !scan.archived.contains(&i.id));
    eval(universe, spec)
        .iter()
        .find_map(|g| g.items.first())
        .copied()
}

/// Who a `--claim` assigns — `--owner`'s single actor (under
/// `--claim` it names the claimer, not a filter), else the
/// `handoff`/`interrupt` fallback (`cli`). Several `--owner`s
/// can't name one claimer, so that's an error.
fn claim_owner(flags: &NextFlags) -> Result<Actor, String> {
    match flags.filter.owner.as_slice() {
        [] => Actor::new(continuity::DEFAULT_ACTOR).map_err(|e| e.to_string()),
        [owner] => Ok(owner.clone()),
        _ => Err("next --claim: pass one --owner (the claimer), not several".into()),
    }
}

/// The one-line human output — `items::compact` + the `⚡`
/// interrupted marker, same as a `list`-format view row.
fn line(store: &impl Store, item: &Item) -> Result<String, String> {
    let mut out = items::compact(item);
    if continuity::is_interrupted(store, item.id)? {
        out.push_str(" ⚡");
    }
    Ok(out)
}

/// The `next` verb — read-only unless `--claim`, which takes
/// the mutating open path + per-verb commit (`chore: claim
/// <ID>`; `--no-commit`/`$MYCO_NO_COMMIT` honored).
pub fn run(
    flags: NextFlags,
    dir: Option<PathBuf>,
    root: Option<PathBuf>,
    json_out: bool,
    no_commit: bool,
) -> Result<i32, String> {
    let owner = flags.claim.then(|| claim_owner(&flags)).transpose()?;
    let (proj, store) = if flags.claim {
        items::open(dir.as_deref(), root.as_deref())?
    } else {
        items::open_read(dir.as_deref(), root.as_deref())?
    };
    let spec = spec(&proj.root, &flags)?;
    let scan = digest::scan(&store)?;
    let Some(item) = pick(&scan, &spec).cloned() else {
        if json_out {
            ops::print_json(&json!({"item": null}));
        } else {
            println!("{}: nothing next — backlog is clear", scan.name);
        }
        return Ok(0);
    };
    let Some(owner) = owner else {
        if json_out {
            ops::print_json(&json!({"item": item}));
        } else {
            println!("{}", line(&store, &item)?);
        }
        return Ok(0);
    };
    let id = item.id;
    let set = itemcmd::SetFlags {
        // identity isn't a transition — an already-claimed item
        // just re-stamps the owner
        status: (item.status != Status::Claimed).then_some(Status::Claimed),
        owner: Some(owner),
        ..Default::default()
    };
    let (item, _location) = itemcmd::mutate_item(&store, id, &set)?;
    let committed = items::commit(&proj.worktree, &format!("chore: claim {id}"), no_commit)?;
    let human = match &item {
        Some(i) => line(&store, i)?,
        None => format!("claimed {id}"),
    };
    items::report(json!({"item": item}), committed, json_out, &human)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use myco_core::{ItemId, ItemKind};

    /// A hermetic root — no myco.toml, no user config leaks.
    fn spec_at(dir: &tempfile::TempDir, flags: &NextFlags) -> ViewSpec {
        temp_env::with_var("MYCO_CONFIG", None::<&str>, || {
            spec(dir.path(), flags).unwrap()
        })
    }

    #[test]
    fn default_spec_is_open_tasks_and_issues_limit_1() {
        let dir = tempfile::tempdir().unwrap();
        let spec = spec_at(&dir, &NextFlags::default());
        assert_eq!(spec.limit, Some(1));
        assert_eq!(spec.filter.kind, [ItemKind::Task, ItemKind::Issue]);
        assert_eq!(spec.filter.status, [Status::Open]);
        assert!(spec.sort.is_empty()); // engine default: pri,id
    }

    #[test]
    fn flags_merge_over_the_default() {
        let flags = NextFlags {
            filter: view::FilterFlags {
                kind: vec![ItemKind::Issue],
                label: vec!["bug".into()],
                ..Default::default()
            },
            sort: vec!["-id".parse().unwrap()],
            claim: false,
        };
        let dir = tempfile::tempdir().unwrap();
        let spec = spec_at(&dir, &flags);
        assert_eq!(spec.filter.kind, [ItemKind::Issue]);
        // unset fields keep the base: status stays [open]
        assert_eq!(spec.filter.status, [Status::Open]);
        assert_eq!(spec.filter.label, ["bug"]);
        assert_eq!(spec.sort.len(), 1);
    }

    #[test]
    fn claim_owner_resolution() {
        let mut flags = NextFlags::default();
        assert_eq!(claim_owner(&flags).unwrap().as_str(), "cli");
        flags.filter.owner = vec!["devin".parse().unwrap()];
        assert_eq!(claim_owner(&flags).unwrap().as_str(), "devin");
        flags.filter.owner = vec!["devin".parse().unwrap(), "human:x".parse().unwrap()];
        assert!(claim_owner(&flags).is_err());
    }

    #[test]
    fn pick_orders_pri_then_id() {
        let items = vec![
            Item::new(ItemId::new(ItemKind::Task, 1), "low").priority(myco_core::Priority::Low),
            Item::new(ItemId::new(ItemKind::Task, 2), "crit")
                .priority(myco_core::Priority::Critical),
            Item::new(ItemId::new(ItemKind::Issue, 1), "hi").priority(myco_core::Priority::High),
        ];
        let scan = digest::Scan {
            name: "p".into(),
            items,
            archived: Default::default(),
        };
        let spec = base_spec();
        assert_eq!(
            pick(&scan, &spec).unwrap().id,
            ItemId::new(ItemKind::Task, 2)
        );
        // a done crit drops out of the open default
        let mut scan2 = digest::Scan {
            name: "p".into(),
            items: scan.items.clone(),
            archived: Default::default(),
        };
        scan2.items[1].status = Status::Done;
        assert_eq!(
            pick(&scan2, &spec).unwrap().id,
            ItemId::new(ItemKind::Issue, 1)
        );
    }
}
