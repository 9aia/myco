//! `DONE.md` — `# Done` plus optional `##` sections of `- T-NNN …`
//! items. No checkboxes: archived items are terminal — `done` or
//! `cancelled` (a `status:` token must say one of them if present;
//! `done` is the default, so canonical emit drops it while
//! `cancelled` stays explicit). The append-only archive
//! `todo done` / `todo cancel` / `item set --status done|cancelled`
//! land in: terminal tasks leave TODO.md but stay readable here —
//! cancelled work keeps its status so the archive says *why* it
//! left. Items nest like BACKLOG.md — a `-` list indented under an
//! item is its `children` (2 spaces per level, canonical).

use markdown::mdast::Node;
use myco_core::{DoneDoc, Item, ItemKind, Section, Status};

use crate::error::{EmitError, ParseError};
use crate::line::{self, LineSpec};
use crate::mdx;

const FILE: &str = "DONE.md";
const SPEC: LineSpec = LineSpec {
    kind: ItemKind::Task,
    default_status: Status::Done,
    file: FILE,
};

/// The statuses DONE.md archives — the machine's terminal pair.
/// `done` is the doc default (token dropped on emit); `cancelled`
/// keeps its explicit token so the archive records the difference.
fn is_archived(status: Status) -> bool {
    matches!(status, Status::Done | Status::Cancelled)
}

fn err(src: &str, node: &Node, msg: impl Into<String>) -> ParseError {
    match node.position() {
        Some(pos) => ParseError::at_position(FILE, src.to_string(), pos, msg),
        None => ParseError::whole_file(FILE, src.to_string(), msg),
    }
}

/// Parse `DONE.md` source into a [`DoneDoc`].
pub fn parse_done(src: &str) -> Result<DoneDoc, ParseError> {
    let src = mdx::normalize(src);
    let ast = mdx::parse(&src).map_err(|m| ParseError::whole_file(FILE, src.clone(), m))?;
    // to_mdast always yields Root; children() keeps it non-fatal
    let mut blocks = ast.children().into_iter().flatten();
    let Some(node @ Node::Heading(h1)) = blocks.next() else {
        return Err(ParseError::whole_file(FILE, src, "expected `# Done` title"));
    };
    if h1.depth != 1 || mdx::single_text(node).map(str::trim) != Some("Done") {
        return Err(err(&src, node, "expected `# Done` title"));
    }
    let mut doc = DoneDoc::default();
    for block in blocks {
        match block {
            Node::Heading(h) if h.depth == 2 => {
                let name = mdx::single_text(block)
                    .ok_or_else(|| err(&src, block, "section heading must be plain text"))?;
                doc.sections.push(Section::named(name.trim()));
            }
            Node::List(list) if !list.ordered => {
                if doc.sections.is_empty() {
                    doc.sections.push(Section::unsectioned());
                }
                if let Some(section) = doc.sections.last_mut() {
                    parse_list(&src, list, section)?;
                }
            }
            other => {
                return Err(err(
                    &src,
                    other,
                    "unexpected block — DONE.md holds `##` sections and `-` items only",
                ));
            }
        }
    }
    Ok(doc)
}

fn parse_list(
    src: &str,
    list: &markdown::mdast::List,
    section: &mut Section,
) -> Result<(), ParseError> {
    for item in &list.children {
        // list children are ListItem by construction; skip anything else
        if let Node::ListItem(li) = item {
            section.items.push(parse_li(src, item, li, 0)?);
        }
    }
    Ok(())
}

/// One `- T-NNN …` list item → an [`Item`]; a nested `-` list
/// under it parses recursively into `children` (bounded by
/// [`line::MAX_ITEM_DEPTH`]).
fn parse_li(
    src: &str,
    node: &Node,
    li: &markdown::mdast::ListItem,
    depth: usize,
) -> Result<Item, ParseError> {
    if depth > line::MAX_ITEM_DEPTH {
        return Err(err(src, node, "items nested too deep"));
    }
    if li.checked.is_some() {
        return Err(err(
            src,
            node,
            "archived items have no checkbox — the file implies done/cancelled",
        ));
    }
    let (head, nested) = line::item_parts(li).map_err(|m| err(src, node, m))?;
    let text =
        mdx::single_text(head).ok_or_else(|| err(src, node, "item line must be plain text"))?;
    let mut parsed = line::parse_text(text, &SPEC).map_err(|m| err(src, node, m))?;
    if !is_archived(parsed.status) {
        return Err(err(
            src,
            node,
            "DONE.md holds done/cancelled items only — live tasks belong in TODO.md/BACKLOG.md",
        ));
    }
    if let Some(list) = nested {
        for child in &list.children {
            if let Node::ListItem(cli) = child {
                parsed.children.push(parse_li(src, child, cli, depth + 1)?);
            }
        }
    }
    Ok(parsed)
}

/// Canonical `DONE.md` — same layout as [`crate::emit_backlog`]
/// with the terminal-only rule. Fails on non-terminal items and
/// the same line-grammar limits as the other ledger docs.
pub fn emit_done(doc: &DoneDoc) -> Result<String, EmitError> {
    let err = |m: &str| EmitError::new(FILE, m);
    let mut out = String::from("# Done\n");
    for (i, section) in doc.sections.iter().enumerate() {
        for item in &section.items {
            check_subtree(item, 0)?;
        }
        out.push('\n');
        match &section.name {
            Some(name) => {
                if name.trim() != *name || name.is_empty() {
                    return Err(err("empty or untrimmed section name"));
                }
                if name.contains('\n') || name.contains('\r') {
                    return Err(err("section name contains a newline"));
                }
                out.push_str("## ");
                out.push_str(&mdx::heading(name));
                out.push('\n');
                if !section.items.is_empty() {
                    out.push('\n');
                }
            }
            None => {
                if i != 0 {
                    return Err(err("unsectioned items must come before the first `##`"));
                }
                if section.items.is_empty() {
                    return Err(err("empty unsectioned section is unrepresentable"));
                }
            }
        }
        for item in &section.items {
            emit_item(&mut out, item, 0)?;
        }
    }
    Ok(out)
}

/// [`line::check_item`] + the terminal-only rule, applied to a
/// whole subtree. Depth matches the parse-side cap so emit can't
/// write a file parse would reject (or overflow the stack on).
fn check_subtree(item: &Item, depth: usize) -> Result<(), EmitError> {
    if depth > line::MAX_ITEM_DEPTH {
        return Err(EmitError::new(
            FILE,
            format!("item {} nested too deep", item.id),
        ));
    }
    line::check_item(item, FILE)?;
    if !is_archived(item.status) {
        return Err(EmitError::new(
            FILE,
            format!("non-terminal item {} in DONE.md", item.id),
        ));
    }
    for c in &item.children {
        check_subtree(c, depth + 1)?;
    }
    Ok(())
}

/// One item line plus its nested children — 2-space indent per
/// level under the parent's bullet.
fn emit_item(out: &mut String, item: &Item, depth: usize) -> Result<(), EmitError> {
    for _ in 0..depth {
        out.push_str("  ");
    }
    out.push_str("- ");
    out.push_str(&mdx::inline(&line::text(item, &SPEC)?));
    out.push('\n');
    for c in &item.children {
        emit_item(out, c, depth + 1)?;
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use myco_core::{Item, ItemId};

    #[test]
    fn roundtrip() {
        let src = "# Done\n\n## 2026-10\n\n- T-034 Old idea · pri:low · #maybe\n- T-040 Another\n";
        let doc = parse_done(src).unwrap();
        assert_eq!(doc.sections.len(), 1);
        assert!(doc.items().all(|i| i.status == Status::Done));
        assert_eq!(emit_done(&doc).unwrap(), src);
    }

    #[test]
    fn unsectioned() {
        let src = "# Done\n\n- T-001 loose\n";
        let doc = parse_done(src).unwrap();
        assert_eq!(doc.sections[0].name, None);
        assert_eq!(emit_done(&doc).unwrap(), src);
        assert_eq!(parse_done(&emit_done(&doc).unwrap()).unwrap(), doc);
    }

    #[test]
    fn explicit_done_ok() {
        let src = "# Done\n\n- T-001 x · status:done\n";
        let doc = parse_done(src).unwrap();
        assert_eq!(doc.sections[0].items[0].status, Status::Done);
        // canonical emit drops the redundant token
        assert_eq!(emit_done(&doc).unwrap(), "# Done\n\n- T-001 x\n");
    }

    #[test]
    fn cancelled_roundtrip() {
        // cancelled is terminal like done but keeps its token —
        // the archive records that the work never happened
        let src = "# Done\n\n- T-001 shipped\n- T-002 wontfix · status:cancelled\n  - T-003 sub · status:cancelled\n";
        let doc = parse_done(src).unwrap();
        assert_eq!(doc.sections[0].items[0].status, Status::Done);
        assert_eq!(doc.sections[0].items[1].status, Status::Cancelled);
        assert_eq!(emit_done(&doc).unwrap(), src);
        // explicit status:done still canonicalizes away
        let src2 = "# Done\n\n- T-001 x · status:done\n- T-002 y · status:cancelled\n";
        assert_eq!(
            emit_done(&parse_done(src2).unwrap()).unwrap(),
            "# Done\n\n- T-001 x\n- T-002 y · status:cancelled\n"
        );
    }

    #[test]
    fn empty_doc() {
        let doc = DoneDoc::default();
        let out = emit_done(&doc).unwrap();
        assert_eq!(out, "# Done\n");
        assert_eq!(parse_done("# Done\n").unwrap(), doc);
    }

    #[test]
    fn parse_errors() {
        for (src, why) in [
            ("# Finish\n", "bad title"),
            ("## Done\n", "h2 title"),
            ("text\n\n# Done\n", "not first"),
            ("# Done\n\ntext\n", "stray para"),
            ("# Done\n\n### x\n", "h3"),
            ("# Done\n\n## s *em*\n", "rich heading"),
            ("# Done\n\n- [ ] T-001 x\n", "checkbox"),
            ("# Done\n\n- [x] T-001 x\n", "checked"),
            ("# Done\n\n- T-001 x · status:open\n", "open status"),
            ("# Done\n\n- T-001 x · status:deferred\n", "deferred status"),
            ("# Done\n\n- T-001 x · status:in-progress\n", "live status"),
            ("# Done\n\n- I-001 x\n", "wrong kind"),
            ("# Done\n\n- T-001\n  cont\n", "multi-line"),
            ("# Done\n\n1. T-001 x\n", "ordered"),
        ] {
            assert!(parse_done(src).is_err(), "{why}: {src:?}");
        }
    }

    #[test]
    fn err_falls_back_to_file_start_without_position() {
        let n = Node::Text(markdown::mdast::Text {
            value: "x".into(),
            position: None,
        });
        let e = err("src", &n, "m");
        assert_eq!((e.line(), e.column()), (1, 1));
    }

    #[test]
    fn nested_items_parse_into_children() {
        let src = "# Done\n\n- T-001 a\n  - T-002 sub\n- T-003 sib\n";
        let doc = parse_done(src).unwrap();
        assert_eq!(doc.sections[0].items.len(), 2);
        let parent = &doc.sections[0].items[0];
        assert_eq!(parent.children.len(), 1);
        assert_eq!(parent.children[0].status, Status::Done);
        assert_eq!(emit_done(&doc).unwrap(), src);
    }

    #[test]
    fn nested_parse_errors() {
        for (src, why) in [
            ("# Done\n\n- T-001 a\n  - [ ] T-002 x\n", "checkbox"),
            (
                "# Done\n\n- T-001 a\n  - T-002 x · status:open\n",
                "open child",
            ),
        ] {
            assert!(parse_done(src).is_err(), "{why}: {src:?}");
        }
        // emit rejects a non-done child too
        let mut doc = DoneDoc::default();
        let child = Item::new(ItemId::new(ItemKind::Task, 2), "sub").status(Status::Open);
        doc.sections.push(Section {
            name: None,
            items: vec![Item::new(ItemId::new(ItemKind::Task, 1), "p")
                .status(Status::Done)
                .child(child)],
        });
        assert!(emit_done(&doc).is_err());
    }

    #[test]
    fn nesting_depth_is_capped() {
        let mut src = String::from("# Done\n\n");
        for d in 0..=line::MAX_ITEM_DEPTH + 2 {
            src.push_str(&"  ".repeat(d));
            src.push_str("- T-001 t\n");
        }
        assert!(parse_done(&src).is_err());
        let mut item = Item::new(ItemId::new(ItemKind::Task, 1), "t").status(Status::Done);
        for _ in 0..line::MAX_ITEM_DEPTH + 2 {
            item = Item::new(ItemId::new(ItemKind::Task, 1), "t")
                .status(Status::Done)
                .child(item);
        }
        let mut doc = DoneDoc::default();
        doc.sections.push(Section::named("s"));
        doc.sections[0].items.push(item);
        assert!(emit_done(&doc).is_err());
    }

    #[test]
    fn emit_errors() {
        let mut doc = DoneDoc::default();
        doc.sections.push(Section {
            name: Some("s".into()),
            items: vec![Item::new(ItemId::new(ItemKind::Task, 1), "x").status(Status::Open)],
        });
        assert!(emit_done(&doc).is_err(), "non-done");

        let mut doc = DoneDoc::default();
        doc.sections.push(Section::named("a"));
        doc.sections.push(Section::unsectioned());
        assert!(emit_done(&doc).is_err(), "unsectioned at idx>0");

        let mut doc = DoneDoc::default();
        doc.sections.push(Section::unsectioned());
        assert!(emit_done(&doc).is_err(), "empty unsectioned");

        let mut doc = DoneDoc::default();
        doc.sections.push(Section {
            name: Some(" x".into()),
            items: vec![],
        });
        assert!(emit_done(&doc).is_err(), "untrimmed name");
    }
}
