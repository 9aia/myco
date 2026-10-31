//! `myco status [--prompt]` — the ledger digest; THE agent-context
//! verb. Human output is compact counts + the in-progress and
//! blocked lists; `--prompt` is a token-sized block (repo, counts
//! one-liner, then items grouped in-progress → blocked → claimed →
//! open-by-priority, one per line, no prose); `--json` is the
//! structured digest — with `--prompt` it's `{"prompt": "<block>"}`.

use std::collections::BTreeSet;
use std::path::PathBuf;

use myco_core::{Item, ItemId, Status};
use serde_json::json;

use crate::{digest, items, ops};

/// Hard cap on `--prompt` lines — the block must stay token-cheap.
pub const PROMPT_MAX_LINES: usize = 60;

/// `<id> <pri> <title> [⚡]` — one prompt line, no prose; `⚡`
/// marks an interrupted item (latest handoff is an interrupt).
fn prompt_item(i: &Item, interrupted: bool) -> String {
    format!(
        "{} {} {}{}",
        i.id,
        i.priority.token(),
        i.title,
        if interrupted { " ⚡" } else { "" }
    )
}

/// The `--prompt` block, hard-capped at [`PROMPT_MAX_LINES`].
/// Sections render in spec order (in-progress → blocked →
/// claimed → open); when the block overflows, items are cut from
/// the *last* non-empty group first — open (always largest)
/// absorbs the budget, then claimed/blocked, with in-progress
/// dropped last. A truncated group keeps a `… +N more <name>`
/// marker; a fully cut group disappears (the counts line still
/// summarizes it).
pub fn prompt_block(scan: &digest::Scan, interrupted: &BTreeSet<ItemId>) -> String {
    let c = digest::counts(scan);
    let mut groups: Vec<(&str, Vec<&Item>, usize)> = vec![
        ("in-progress", scan.in_status(Status::InProgress), 0),
        ("blocked", scan.in_status(Status::Blocked), 0),
        ("claimed", scan.in_status(Status::Claimed), 0),
        ("open", scan.in_status(Status::Open), 0),
    ];
    let open = &mut groups[3].1;
    open.sort_by(|a, b| digest::pri_then_id(a, b));

    // render cost: repo line + per non-empty group (header +
    // shown items + marker when truncated)
    let cost = |groups: &[(&str, Vec<&Item>, usize)]| {
        1 + groups
            .iter()
            .map(|(_, shown, hidden)| {
                if shown.is_empty() {
                    0
                } else {
                    1 + shown.len() + usize::from(*hidden > 0)
                }
            })
            .sum::<usize>()
    };
    // shrink tail-first until the whole block fits
    while cost(&groups) > PROMPT_MAX_LINES {
        match groups
            .iter_mut()
            .rev()
            .find(|(_, shown, _)| !shown.is_empty())
        {
            Some((_, shown, hidden)) => {
                shown.pop();
                *hidden += 1;
            }
            None => break,
        }
    }

    let mut out = vec![format!("repo:{} {}", scan.name, c.oneline())];
    for (name, shown, hidden) in &groups {
        if shown.is_empty() {
            continue;
        }
        out.push(format!("{name}:"));
        out.extend(
            shown
                .iter()
                .map(|i| prompt_item(i, interrupted.contains(&i.id))),
        );
        if *hidden > 0 {
            out.push(format!("… +{hidden} more {name}"));
        }
    }
    debug_assert!(out.len() <= PROMPT_MAX_LINES);
    out.join("\n")
}

/// The `--json` digest — counts + the active groups as full items.
pub fn digest_json(scan: &digest::Scan) -> serde_json::Value {
    let c = digest::counts(scan);
    let group = |s: Status| {
        scan.in_status(s)
            .iter()
            .map(|i| serde_json::to_value(i).unwrap_or(json!(null)))
            .collect::<Vec<_>>()
    };
    json!({
        "repo": scan.name,
        "counts": c.to_json(),
        "in-progress": group(Status::InProgress),
        "blocked": group(Status::Blocked),
        "claimed": group(Status::Claimed),
        "open": group(Status::Open),
    })
}

/// The status digest + `interrupted` id list — the `--json`
/// payload, shared with `myco serve`.
pub fn status_json(store: &myco_store_md::MdStore) -> Result<serde_json::Value, String> {
    let scan = digest::scan(store)?;
    let interrupted = crate::continuity::interrupted_set(store, scan.live())?;
    let mut v = digest_json(&scan);
    v["interrupted"] = json!(interrupted
        .iter()
        .map(|i| i.to_string())
        .collect::<Vec<_>>());
    Ok(v)
}

/// The `--prompt` block payload — `{"prompt": "<block>"}` for
/// `myco serve`, where the digest isn't a useful shape.
pub fn prompt_json(store: &myco_store_md::MdStore) -> Result<serde_json::Value, String> {
    let scan = digest::scan(store)?;
    let interrupted = crate::continuity::interrupted_set(store, scan.live())?;
    Ok(json!({"prompt": prompt_block(&scan, &interrupted)}))
}

/// The `status` verb.
pub fn run(
    dir: Option<PathBuf>,
    root: Option<PathBuf>,
    prompt: bool,
    json_out: bool,
) -> Result<i32, String> {
    let (_proj, store) = items::open_read(dir.as_deref(), root.as_deref())?;
    let scan = digest::scan(&store)?;
    let interrupted = crate::continuity::interrupted_set(&store, scan.live())?;
    if prompt {
        if json_out {
            ops::print_json(&json!({"prompt": prompt_block(&scan, &interrupted)}));
        } else {
            println!("{}", prompt_block(&scan, &interrupted));
        }
        return Ok(0);
    }
    if json_out {
        let mut v = digest_json(&scan);
        v["interrupted"] = json!(interrupted
            .iter()
            .map(|i| i.to_string())
            .collect::<Vec<_>>());
        ops::print_json(&v);
        return Ok(0);
    }
    // human: counts + the two groups an agent asks about first
    let c = digest::counts(&scan);
    println!("{} — project ledger", scan.name);
    println!("{}", c.oneline());
    for (name, status) in [
        ("in progress", Status::InProgress),
        ("blocked", Status::Blocked),
    ] {
        let list = scan.in_status(status);
        if !list.is_empty() {
            println!("{name}:");
            for i in list {
                println!(
                    "  {}{}",
                    items::compact(i),
                    if interrupted.contains(&i.id) {
                        " ⚡"
                    } else {
                        ""
                    }
                );
            }
        }
    }
    Ok(0)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use myco_core::{ItemId, ItemKind, Priority};
    use std::collections::BTreeSet;

    fn scan_with(items: Vec<Item>) -> digest::Scan {
        digest::Scan {
            name: "myco".into(),
            items,
            archived: BTreeSet::new(),
        }
    }

    #[test]
    fn prompt_block_groups_and_caps() {
        let mut v = vec![
            Item::new(ItemId::new(ItemKind::Task, 2), "work").status(Status::InProgress),
            Item::new(ItemId::new(ItemKind::Issue, 1), "stuck").status(Status::Blocked),
            Item::new(ItemId::new(ItemKind::Task, 5), "owned").status(Status::Claimed),
            Item::new(ItemId::new(ItemKind::Task, 1), "hi").priority(Priority::Critical),
            Item::new(ItemId::new(ItemKind::Task, 3), "mid"),
            Item::new(ItemId::new(ItemKind::Task, 4), "done").status(Status::Done),
            Item::new(ItemId::new(ItemKind::Task, 6), "parked").status(Status::Deferred),
        ];
        let block = prompt_block(&scan_with(v.clone()), &BTreeSet::new());
        let lines: Vec<&str> = block.lines().collect();
        assert_eq!(
            lines[0],
            "repo:myco tasks 2o/1c/1ip/0b backlog 1 issues 1o p 0o pl 0o adr 0o"
        );
        let whole = lines.join("\n");
        let ip = lines.iter().position(|l| *l == "in-progress:").unwrap();
        let bl = lines.iter().position(|l| *l == "blocked:").unwrap();
        let cl = lines.iter().position(|l| *l == "claimed:").unwrap();
        let op = lines.iter().position(|l| *l == "open:").unwrap();
        assert!(ip < bl && bl < cl && cl < op);
        // open group is crit-first
        assert!(whole.contains("T-001 crit hi"));
        assert!(lines.len() <= PROMPT_MAX_LINES);
        // done/deferred never appear
        assert!(!whole.contains("done") && !whole.contains("parked"));

        // overflow: 70 open items → capped at 60 lines with a marker
        v.clear();
        for n in 1..=70 {
            v.push(Item::new(
                ItemId::new(ItemKind::Task, n),
                format!("task {n}"),
            ));
        }
        let block = prompt_block(&scan_with(v), &BTreeSet::new());
        let lines: Vec<&str> = block.lines().collect();
        assert!(lines.len() <= PROMPT_MAX_LINES, "{}", lines.len());
        assert!(block.contains("… +"), "{}", block);
    }

    #[test]
    fn prompt_block_caps_the_whole_block() {
        // 30 in-progress + 30 blocked + 20 claimed + 20 open —
        // >60 non-open lines must not overflow the cap; open is
        // cut first, claimed next, in-progress last
        let mut v = Vec::new();
        for n in 1..=30 {
            v.push(
                Item::new(ItemId::new(ItemKind::Task, n), format!("wip {n}"))
                    .status(Status::InProgress),
            );
        }
        for n in 31..=60 {
            v.push(
                Item::new(ItemId::new(ItemKind::Task, n), format!("stuck {n}"))
                    .status(Status::Blocked),
            );
        }
        for n in 61..=80 {
            v.push(
                Item::new(ItemId::new(ItemKind::Task, n), format!("owned {n}"))
                    .status(Status::Claimed),
            );
        }
        for n in 81..=100 {
            v.push(Item::new(
                ItemId::new(ItemKind::Task, n),
                format!("open {n}"),
            ));
        }
        let block = prompt_block(&scan_with(v), &BTreeSet::new());
        let lines: Vec<&str> = block.lines().collect();
        assert!(lines.len() <= PROMPT_MAX_LINES, "{}", lines.len());
        // in-progress is the last section cut — survives whole
        assert!(block.contains("in-progress:"), "{block}");
        assert!(block.contains("T-030"), "{block}");
        // open + claimed were cut entirely (tail-first)
        assert!(!block.contains("open:"), "{block}");
        assert!(!block.contains("claimed:"), "{block}");
        assert!(!block.contains("T-081"), "{block}");
        // blocked shows a truncated tail + marker
        assert!(block.contains("… +"), "{block}");
    }

    #[test]
    fn prompt_block_no_open_items() {
        // in-progress/blocked but nothing open → no "open:" section
        let v = vec![
            Item::new(ItemId::new(ItemKind::Task, 1), "wip").status(Status::InProgress),
            Item::new(ItemId::new(ItemKind::Task, 2), "stuck").status(Status::Blocked),
        ];
        let block = prompt_block(&scan_with(v), &BTreeSet::new());
        assert!(block.contains("in-progress:"), "{block}");
        assert!(!block.contains("open:"), "{block}");
    }

    #[test]
    fn digest_json_shape() {
        let s = scan_with(vec![
            Item::new(ItemId::new(ItemKind::Task, 1), "x").status(Status::Blocked)
        ]);
        let j = digest_json(&s);
        assert_eq!(j["repo"], "myco");
        assert_eq!(j["counts"]["tasks"]["blocked"], 1);
        assert_eq!(j["blocked"].as_array().unwrap()[0]["id"], "T-001");
        assert!(j["open"].as_array().unwrap().is_empty());
    }
}
