//! links — the md-grammar half of the link index (P-005).
//!
//! [`extract_file`] classifies a `project/` file by path and
//! produces its `refs::extract` edges; `index add`/`rebuild` feed
//! it to `myco_ops::links::sync`. Kept in the composition root
//! because it's the file spec speaking — ops stays grammar-free,
//! and phase-3 consumers (`item show`, `serve`) can share this
//! same read side.

use camino::Utf8Path;
use myco_core::{refs, Edge, Item, ItemId, ItemKind};
use myco_store_md::{
    item_dir, parse_backlog, parse_handoff_file, parse_handoff_filename, parse_item_file,
    parse_todo, HANDOFFS_DIR,
};

/// The edges one `project/` file declares — classified by the md
/// grammar:
///
/// - `TODO.md` / `BACKLOG.md` — one extract per task item
///   (frontmatter `blocks:` + title mentions), children included.
/// - `handoffs/[archive/]<ID>.md` — the filename `handoff`
///   self-edge plus mentions inside entry notes.
/// - `<dir>/[archive/]<slug>.md` — frontmatter `blocks:`/
///   `supersedes:`/`superseded_by:` + body mentions.
/// - anything else (`index.md`, notes) — zero edges; the file is
///   still fingerprinted so `sweep` tracks it.
pub fn extract_file(rel: &Utf8Path, src: &str) -> Result<Vec<Edge>, String> {
    let mut edges = Vec::new();
    match rel.as_str() {
        "TODO.md" => {
            let doc = parse_todo(src).map_err(|e| format!("{rel}: {e}"))?;
            for i in doc.items() {
                push_item(&mut edges, i);
            }
        }
        "BACKLOG.md" => {
            let doc = parse_backlog(src).map_err(|e| format!("{rel}: {e}"))?;
            for i in doc.items() {
                push_item(&mut edges, i);
            }
        }
        _ => {
            if let Some(id) = handoff_id(rel) {
                edges.push(Edge::handoff(id));
                let (_, entries) =
                    parse_handoff_file(src, rel.as_str()).map_err(|e| format!("{rel}: {e}"))?;
                let notes = entries
                    .iter()
                    .map(|e| e.note.as_str())
                    .collect::<Vec<_>>()
                    .join("\n");
                edges.extend(refs::extract(&notes, None, id));
            } else if let Some(kind) = item_kind(rel) {
                let f =
                    parse_item_file(src, kind, rel.as_str()).map_err(|e| format!("{rel}: {e}"))?;
                edges.extend(refs::extract(&f.body, Some(&f.meta), f.meta.id));
            }
        }
    }
    Ok(edges)
}

/// One ledger item's edges — the whole subtree, since children
/// are full items.
fn push_item(edges: &mut Vec<Edge>, item: &Item) {
    for i in item.subtree() {
        edges.extend(refs::extract(&i.title, Some(i), i.id));
    }
}

/// `handoffs/[archive/]<ID>.md` → the item id, when the file is
/// under `handoffs/` and its name parses as an id.
fn handoff_id(rel: &Utf8Path) -> Option<ItemId> {
    if rel.components().next()?.as_str() != HANDOFFS_DIR {
        return None;
    }
    parse_handoff_filename(rel.file_name()?)
}

/// The item kind a path's first component names — covers both
/// `<dir>/x.md` and `<dir>/archive/x.md`.
fn item_kind(rel: &Utf8Path) -> Option<ItemKind> {
    let dir = rel.components().next()?.as_str();
    [
        ItemKind::Issue,
        ItemKind::Proposal,
        ItemKind::Plan,
        ItemKind::Adr,
    ]
    .into_iter()
    .find(|k| item_dir(*k) == Some(dir))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use myco_core::EdgeKind;

    fn tid(n: u32) -> ItemId {
        ItemId::new(ItemKind::Task, n)
    }

    #[test]
    fn todo_items_contribute_blocks_and_mentions() {
        let src = "# TODO — x\n\n- [ ] T-001 alpha\n- [ ] T-002 beta, see T-001 · blocks:T-003\n- [ ] T-003 gamma\n";
        let edges = extract_file(Utf8Path::new("TODO.md"), src).unwrap();
        assert!(
            edges.contains(&Edge::new(tid(2), tid(1), EdgeKind::References)),
            "{edges:?}"
        );
        assert!(
            edges.contains(&Edge::new(tid(2), tid(3), EdgeKind::Blocks)),
            "{edges:?}"
        );
    }

    #[test]
    fn handoff_file_is_a_self_edge_plus_mentions() {
        let src = "+++\nitem = \"T-001\"\n+++\n\n# Handoffs — T-001\n\n## 2026-10-29 · devin\nDone: x. Next: I-003.\n";
        let edges = extract_file(Utf8Path::new("handoffs/T-001.md"), src).unwrap();
        assert!(edges.contains(&Edge::handoff(tid(1))), "{edges:?}");
        assert!(
            edges.contains(&Edge::new(
                tid(1),
                ItemId::new(ItemKind::Issue, 3),
                EdgeKind::References
            )),
            "{edges:?}"
        );
    }

    #[test]
    fn item_file_edges_come_from_meta_and_body() {
        let src = "+++\nid = \"I-003\"\ntitle = \"leak\"\nstatus = \"open\"\ncreated = \"2026-01-01\"\n+++\n\n# I-003 leak\n\nRelated to T-007.\n";
        let edges = extract_file(Utf8Path::new("issues/issue-0003-leak.md"), src).unwrap();
        assert!(
            edges.contains(&Edge::new(
                ItemId::new(ItemKind::Issue, 3),
                tid(7),
                EdgeKind::References
            )),
            "{edges:?}"
        );
        // archive scope classifies the same
        let edges = extract_file(Utf8Path::new("issues/archive/issue-0003-leak.md"), src).unwrap();
        assert!(!edges.is_empty());
    }

    #[test]
    fn unrecognized_files_yield_no_edges() {
        assert!(extract_file(Utf8Path::new("index.md"), "# index\nT-001").is_ok());
        let edges = extract_file(Utf8Path::new("index.md"), "# index\nT-001").unwrap();
        assert!(edges.is_empty());
        // a T-NNN filename outside handoffs/ is not a handoff edge
        let edges = extract_file(Utf8Path::new("notes/T-001.md"), "hi").unwrap();
        assert!(edges.is_empty());
    }
}
