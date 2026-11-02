//! `myco epic [name]` / `myco milestone [name]` — grouping views
//! over item metadata, driven by the named views of the same
//! names (`[views.epic]`/`[views.milestone]` config shadows the
//! presets). Bare lists every group with its item count; a name
//! renders that group's board (`epic v2` ≡ `board --epic v2`).

use std::path::PathBuf;

use myco_core::{eval, UNGROUPED};
use serde_json::json;

use crate::{board, digest, items, ops, view};

/// Which named view the group verb resolves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    /// `item.epic` — `myco epic` resolves the `epic` view.
    Epic,
    /// `item.milestone` — `myco milestone` resolves `milestone`.
    Milestone,
}

impl Group {
    /// The verb/view/field name in output (`epic`, `milestone`).
    pub fn name(self) -> &'static str {
        match self {
            Group::Epic => "epic",
            Group::Milestone => "milestone",
        }
    }
}

/// The shared `epic`/`milestone` verb body. `all` lifts the board
/// row cap when a name renders a group board.
pub fn run(
    group: Group,
    name: Option<String>,
    md: bool,
    all: bool,
    dir: Option<PathBuf>,
    root: Option<PathBuf>,
    json_out: bool,
) -> Result<i32, String> {
    let (proj, store) = items::open_read(dir.as_deref(), root.as_deref())?;
    let scan = digest::scan(&store)?;
    match name {
        None => {
            // the named view's buckets, summarized to
            // `name  count` lines — empty and `ungrouped`
            // buckets aren't groups
            let spec = view::scoped_spec(&proj.root, group.name(), None, None)?;
            let groups = eval(scan.live(), &spec);
            let counts: Vec<(&str, usize)> = groups
                .iter()
                .filter(|g| !g.items.is_empty())
                .filter_map(|g| {
                    g.label
                        .as_deref()
                        .filter(|l| *l != UNGROUPED)
                        .map(|l| (l, g.items.len()))
                })
                .collect();
            if json_out {
                ops::print_json(&json!({
                    "group": group.name(),
                    "groups": counts
                        .iter()
                        .map(|(name, count)| json!({"name": name, "count": count}))
                        .collect::<Vec<_>>(),
                }));
            } else if counts.is_empty() {
                println!(
                    "{}: no {}s — set one via `myco item set <id> --{} <name>`",
                    scan.name,
                    group.name(),
                    group.name()
                );
            } else {
                for (name, count) in counts {
                    println!("{name}  {count}");
                }
            }
            Ok(0)
        }
        Some(name) => {
            if !myco_core::is_valid_group_name(&name) {
                return Err(format!("invalid {} name {name:?}", group.name()));
            }
            // `epic v2` ≡ `board --epic v2` — the board view
            // scoped to this group
            let (epic, milestone) = match group {
                Group::Epic => (Some(name.clone()), None),
                Group::Milestone => (None, Some(name.clone())),
            };
            let spec = view::scoped_spec(&proj.root, "board", epic, milestone)?;
            let groups = eval(scan.live(), &spec);
            let interrupted = crate::continuity::interrupted_set(
                &store,
                groups.iter().flat_map(|g| g.items.iter().copied()),
            )?;
            board::emit(&board::Emit {
                repo: &scan.name,
                view: "board",
                spec: &spec,
                groups: &groups,
                interrupted: &interrupted,
                md,
                all,
                json_out,
                empty_msg: &format!("no items in {} {name:?}", group.name()),
            })
        }
    }
}
