//! `myco handoff` / `myco resume` — the continuity pair. At
//! session end `handoff` appends a `## <today> · <actor>` entry
//! plus note to `handoffs/<ID>.md` (append-only, one file per
//! item, uniform across kinds). At session start `resume` emits
//! the minimal restart context — the item, its `blocks:` dep
//! titles, the latest handoff entries, the open checklist — sized
//! for a prompt, the single-item counterpart of `status --prompt`.
//!
//! sepia owns session lifecycle; it calls these verbs from hooks.
//! `--session` is provenance only: a sepia session id,
//! `cli:<pid>`, whatever — myco records it, never interprets it.

use std::path::PathBuf;

use myco_core::{is_valid_session, Actor, HandoffEntry, ItemId};
use myco_store::Store;
use serde_json::json;

use crate::{items, ops};

/// `--actor` fallback — the local CLI wrote it.
const DEFAULT_ACTOR: &str = "cli";

/// Handoff entries shown by default (`--last` overrides).
const DEFAULT_LAST: usize = 10;

/// `--note` inline, or `-` for stdin.
fn read_note(note: &str) -> Result<String, String> {
    let note = if note == "-" {
        use std::io::Read;
        let mut s = String::new();
        std::io::stdin()
            .read_to_string(&mut s)
            .map_err(|e| format!("read --note from stdin: {e}"))?;
        s
    } else {
        note.to_string()
    };
    let note = note.trim().to_string();
    if note.is_empty() {
        return Err("handoff: --note is empty".into());
    }
    Ok(note)
}

/// The `handoff` verb — the item must exist somewhere in the
/// ledger (any kind, live or archived).
#[allow(clippy::too_many_arguments)]
pub fn handoff(
    id: ItemId,
    note: &str,
    actor: Option<Actor>,
    session: Option<String>,
    dir: Option<PathBuf>,
    root: Option<PathBuf>,
    json_out: bool,
    no_commit: bool,
) -> Result<i32, String> {
    let (proj, store) = items::open(dir.as_deref(), root.as_deref())?;
    if items::find_item(&store, id)?.is_none() {
        return Err(format!("{id}: no such item — handoffs attach to items"));
    }
    if let Some(s) = &session {
        if !is_valid_session(s) {
            return Err(format!(
                "invalid --session {s:?} — no empty, no `·`, no newlines"
            ));
        }
    }
    let entry = HandoffEntry {
        date: items::today(),
        actor: match actor {
            Some(a) => a,
            None => Actor::new(DEFAULT_ACTOR).map_err(|e| e.to_string())?,
        },
        session,
        note: read_note(note)?,
    };
    store
        .append_handoff(id, &entry)
        .map_err(|e| e.to_string())?;
    // the file lands wherever it already lived
    let live = format!("handoffs/{id}.md");
    let rel = if proj.worktree.join(&live).exists() {
        live
    } else {
        format!("handoffs/archive/{id}.md")
    };
    let committed = items::commit(&proj.worktree, &format!("chore: handoff {id}"), no_commit)?;
    items::report(
        json!({"id": id.to_string(), "path": rel, "entry": entry}),
        committed,
        json_out,
        &format!("handoff {id} → {rel}"),
    )
}

/// Unchecked `- [ ]` checkbox items in a markdown body — the open
/// checklist `resume` surfaces (issue bodies, plan phases).
fn open_checklist(body: &str) -> Vec<String> {
    body.lines()
        .filter_map(|l| {
            let t = l.trim_start();
            for m in ["- [ ]", "* [ ]", "+ [ ]"] {
                if let Some(rest) = t.strip_prefix(m) {
                    if rest.is_empty() || rest.starts_with(char::is_whitespace) {
                        return Some(rest.trim().to_string());
                    }
                }
            }
            None
        })
        .collect()
}

/// The `resume` verb — `--json` is
/// `{item, location, blocks_titles, handoffs[], open_checklist}`;
/// human output is a compact prompt-ready block.
pub fn resume(
    id: ItemId,
    last: Option<usize>,
    dir: Option<PathBuf>,
    root: Option<PathBuf>,
    json_out: bool,
) -> Result<i32, String> {
    let (_proj, store) = items::open(dir.as_deref(), root.as_deref())?;
    let Some((item, location, body)) = items::find_item(&store, id)? else {
        return Err(format!("{id}: no such item"));
    };
    let mut blocks_titles = Vec::new();
    for b in &item.blocks {
        let title = items::find_item(&store, *b)?.map(|(i, _, _)| i.title);
        blocks_titles.push(json!({"id": b.to_string(), "title": title}));
    }
    let entries = store.read_handoffs(id).map_err(|e| e.to_string())?;
    let total = entries.len();
    let n = last.unwrap_or(DEFAULT_LAST);
    let shown: Vec<&HandoffEntry> = entries
        .iter()
        .rev()
        .take(n)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    let checklist = body.as_deref().map(open_checklist).unwrap_or_default();
    if json_out {
        ops::print_json(&json!({
            "item": item,
            "location": location,
            "blocks_titles": blocks_titles,
            "handoffs": shown,
            "handoffs_total": total,
            "open_checklist": checklist,
        }));
        return Ok(0);
    }
    // prompt-ready block — compact item line first; sections only
    // when they carry context
    println!("{}", items::compact(&item));
    println!("in: {location}");
    if !blocks_titles.is_empty() {
        println!("blocks:");
        for b in &blocks_titles {
            println!(
                "  {} {}",
                b["id"].as_str().unwrap_or_default(),
                b["title"].as_str().unwrap_or("(missing)")
            );
        }
    }
    if !shown.is_empty() {
        println!("handoffs:");
        for e in &shown {
            let mut head = format!("{} · {}", e.date, e.actor);
            if let Some(s) = &e.session {
                head.push_str(&format!(" · session:{s}"));
            }
            println!("{head}");
            for line in e.note.lines() {
                println!("  {line}");
            }
            println!();
        }
        if total > shown.len() {
            println!("… +{} earlier entries", total - shown.len());
        }
    }
    if !checklist.is_empty() {
        println!("open checklist:");
        for c in &checklist {
            println!("  - [ ] {c}");
        }
    }
    Ok(0)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn checklist_extraction() {
        let body = "## Checklist\n\n- [ ] do a\n  - [x] done\n* [ ] star\n+ [ ] plus\n- [ ]not-a-box\n- plain\n\n- [ ]\n";
        assert_eq!(open_checklist(body), vec!["do a", "star", "plus", ""]);
        assert!(open_checklist("no items\n").is_empty());
    }
}
