//! `myco board [--md]` — kanban of the current repo's ledger:
//! Backlog | Open | Claimed | In Progress | Blocked columns (done
//! items leave the ledger, so there's no Done column). Human
//! output is a `tabled` grid; `--md` is the same board as a
//! markdown table for embedding in docs; `--json` is the columns
//! map.

use std::path::PathBuf;

use myco_core::{Item, Status};
use serde_json::json;
use tabled::settings::Style;
use tabled::{Table, Tabled};

use crate::{digest, items, ops};

/// Column order — a `deferred` item is a backlog item wherever it
/// lives, and `done` has no column (done leaves the ledger).
pub const COLUMNS: [(&str, Option<Status>); 5] = [
    ("backlog", Some(Status::Deferred)),
    ("open", Some(Status::Open)),
    ("claimed", Some(Status::Claimed)),
    ("in-progress", Some(Status::InProgress)),
    ("blocked", Some(Status::Blocked)),
];

/// One board row — five card cells, any may be empty.
#[derive(Tabled)]
struct Row {
    #[tabled(rename = "Backlog")]
    backlog: String,
    #[tabled(rename = "Open")]
    open: String,
    #[tabled(rename = "Claimed")]
    claimed: String,
    #[tabled(rename = "In Progress")]
    in_progress: String,
    #[tabled(rename = "Blocked")]
    blocked: String,
}

/// Card text — `T-012 the title`, with an optional prefix
/// (`myco:`) for org boards.
fn card(item: &Item, prefix: Option<&str>) -> String {
    let id = match prefix {
        Some(p) => format!("{p}:{}", item.id),
        None => item.id.to_string(),
    };
    if item.title.is_empty() {
        id
    } else {
        format!("{id} {}", item.title)
    }
}

/// Bucket live items into the five columns (in-column order is by
/// priority then id — the most urgent cards float to the top).
fn columns<'a>(items: impl Iterator<Item = &'a Item>) -> [Vec<&'a Item>; 5] {
    let mut cols: [Vec<&'a Item>; 5] = Default::default();
    for i in items {
        let idx = COLUMNS.iter().position(|(_, s)| *s == Some(i.status));
        if let Some(idx) = idx {
            cols[idx].push(i);
        }
    }
    for col in &mut cols {
        col.sort_by(|a, b| digest::pri_then_id(a, b));
    }
    cols
}

/// Render the board. `prefix` puts `repo:` on every card (org).
pub fn render<'a>(items: impl Iterator<Item = &'a Item>, prefix: Option<&str>, md: bool) -> String {
    let cols = columns(items);
    let cards: [Vec<String>; 5] =
        std::array::from_fn(|c| cols[c].iter().map(|i| card(i, prefix)).collect());
    render_cards(&cards, md)
}

/// Render five columns of card strings — shared with `org board`,
/// which builds mixed-repo cards itself.
pub fn render_cards(cards: &[Vec<String>; 5], md: bool) -> String {
    let height = cards.iter().map(Vec::len).max().unwrap_or(0);
    let mut rows = Vec::with_capacity(height);
    for r in 0..height {
        let cell = |c: usize| cards[c].get(r).cloned().unwrap_or_default();
        rows.push(Row {
            backlog: cell(0),
            open: cell(1),
            claimed: cell(2),
            in_progress: cell(3),
            blocked: cell(4),
        });
    }
    let mut table = Table::new(rows);
    if md {
        table.with(Style::markdown());
    }
    table.to_string()
}

/// Bucket `(label, item)` pairs into the five columns, sorting
/// each by priority — org cards carry their repo prefix already.
pub fn cards_by_column<'a>(items: impl Iterator<Item = (String, &'a Item)>) -> [Vec<String>; 5] {
    let mut cols: [Vec<(&'a Item, String)>; 5] = Default::default();
    for (label, i) in items {
        if let Some(idx) = COLUMNS.iter().position(|(_, s)| *s == Some(i.status)) {
            cols[idx].push((i, label));
        }
    }
    std::array::from_fn(|c| {
        let mut col = std::mem::take(&mut cols[c]);
        col.sort_by(|(a, _), (b, _)| digest::pri_then_id(a, b));
        col.into_iter().map(|(_, l)| l).collect()
    })
}

/// The `--json` columns map — column name → full items.
pub fn columns_json<'a>(
    items: impl Iterator<Item = &'a Item>,
    prefix: Option<&str>,
) -> serde_json::Value {
    let cols = columns(items);
    let mut map = serde_json::Map::new();
    for (i, (name, _)) in COLUMNS.iter().enumerate() {
        map.insert(
            (*name).to_string(),
            json!(cols[i]
                .iter()
                .map(|it| {
                    let mut v = serde_json::to_value(it).unwrap_or(json!(null));
                    if let (Some(p), Some(obj)) = (prefix, v.as_object_mut()) {
                        obj.insert("repo".into(), json!(p));
                    }
                    v
                })
                .collect::<Vec<_>>()),
        );
    }
    json!(map)
}

/// The `board` verb. `--epic`/`--milestone` scope the columns to
/// items in that group (both set = AND).
pub fn run(
    dir: Option<PathBuf>,
    root: Option<PathBuf>,
    md: bool,
    epic: Option<String>,
    milestone: Option<String>,
    json_out: bool,
) -> Result<i32, String> {
    let (_proj, store) = items::open(dir.as_deref(), root.as_deref())?;
    let scan = digest::scan(&store)?;
    let live = scan.live().filter(|i| {
        epic.as_deref().is_none_or(|e| i.epic.as_deref() == Some(e))
            && milestone
                .as_deref()
                .is_none_or(|m| i.milestone.as_deref() == Some(m))
    });
    if json_out {
        ops::print_json(&columns_json(live, None));
        return Ok(0);
    }
    let board = render(live, None, md);
    if board.trim().is_empty() {
        println!("{}: empty board — `myco todo add` something", scan.name);
    } else {
        println!("{board}");
    }
    Ok(0)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use myco_core::{ItemId, ItemKind, Priority};

    fn items() -> Vec<Item> {
        vec![
            Item::new(ItemId::new(ItemKind::Task, 1), "parked").status(Status::Deferred),
            Item::new(ItemId::new(ItemKind::Task, 2), "plain"),
            Item::new(ItemId::new(ItemKind::Task, 3), "urgent").priority(Priority::Critical),
            Item::new(ItemId::new(ItemKind::Issue, 1), "owned").status(Status::Claimed),
            Item::new(ItemId::new(ItemKind::Task, 4), "wip").status(Status::InProgress),
            Item::new(ItemId::new(ItemKind::Task, 5), "stuck").status(Status::Blocked),
            Item::new(ItemId::new(ItemKind::Task, 6), "gone").status(Status::Done),
        ]
    }

    #[test]
    fn columns_bucket_and_sort() {
        let v = items();
        let cols = columns(v.iter());
        assert_eq!(cols[0][0].id, ItemId::new(ItemKind::Task, 1));
        // open column: crit before med
        assert_eq!(
            cols[1].iter().map(|i| i.id.number()).collect::<Vec<_>>(),
            [3, 2]
        );
        assert_eq!(cols[2][0].id, ItemId::new(ItemKind::Issue, 1));
        assert_eq!(cols[3][0].id, ItemId::new(ItemKind::Task, 4));
        assert_eq!(cols[4][0].id, ItemId::new(ItemKind::Task, 5));
        // done items land nowhere
        assert!(cols.iter().flatten().all(|i| i.status != Status::Done));
    }

    #[test]
    fn render_grid_and_md() {
        let v = items();
        let grid = render(v.iter(), None, false);
        assert!(grid.contains("Backlog"));
        assert!(grid.contains("In Progress"));
        assert!(grid.contains("T-004 wip"));
        let md = render(v.iter(), Some("myco"), true);
        assert!(md.starts_with('|'));
        assert!(md.contains("myco:T-001 parked"));
        assert!(md.contains("myco:I-001 owned"));
        // markdown table: header + separator + rows
        assert!(md.lines().any(|l| l.contains("---")));
    }

    #[test]
    fn columns_json_map() {
        let v = items();
        let j = columns_json(v.iter(), Some("myco"));
        assert_eq!(j["open"].as_array().unwrap().len(), 2);
        assert_eq!(j["open"][0]["id"], "T-003");
        assert_eq!(j["open"][0]["repo"], "myco");
        assert!(j["blocked"].as_array().unwrap()[0]["id"] == "T-005");
        assert!(j["backlog"].as_array().unwrap().len() == 1);
    }
}
