//! `BACKLOG.md` — `# Backlog` plus optional `##` sections of
//! `- T-NNN …` items. No checkboxes: backlog items are `Deferred`
//! (a `status:` token must say `deferred` if present).

use markdown::mdast::Node;
use myco_core::{BacklogDoc, ItemKind, Section, Status};

use crate::error::{EmitError, ParseError};
use crate::line::{self, LineSpec};
use crate::mdx;

const FILE: &str = "BACKLOG.md";
const SPEC: LineSpec = LineSpec {
    kind: ItemKind::Task,
    default_status: Status::Deferred,
    file: FILE,
};

fn err(src: &str, node: &Node, msg: impl Into<String>) -> ParseError {
    match node.position() {
        Some(pos) => ParseError::at_position(FILE, src.to_string(), pos, msg),
        None => ParseError::whole_file(FILE, src.to_string(), msg),
    }
}

/// Parse `BACKLOG.md` source into a [`BacklogDoc`].
pub fn parse_backlog(src: &str) -> Result<BacklogDoc, ParseError> {
    let src = mdx::normalize(src);
    let ast = mdx::parse(&src).map_err(|m| ParseError::whole_file(FILE, src.clone(), m))?;
    // to_mdast always yields Root; children() keeps it non-fatal
    let mut blocks = ast.children().into_iter().flatten();
    let Some(node @ Node::Heading(h1)) = blocks.next() else {
        return Err(ParseError::whole_file(
            FILE,
            src,
            "expected `# Backlog` title",
        ));
    };
    if h1.depth != 1 || mdx::single_text(node).map(str::trim) != Some("Backlog") {
        return Err(err(&src, node, "expected `# Backlog` title"));
    }
    let mut doc = BacklogDoc::default();
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
                let section = doc.sections.last_mut().expect("non-empty");
                parse_list(&src, list, section)?;
            }
            other => {
                return Err(err(
                    &src,
                    other,
                    "unexpected block — BACKLOG.md holds `##` sections and `-` items only",
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
            if li.checked.is_some() {
                return Err(err(
                    src,
                    item,
                    "backlog items have no checkbox — they're deferred",
                ));
            }
            if li.children.len() != 1 || !matches!(li.children[0], Node::Paragraph(_)) {
                return Err(err(src, item, "item must be a single line"));
            }
            let text = mdx::single_text(&li.children[0])
                .ok_or_else(|| err(src, item, "item line must be plain text"))?;
            let parsed = line::parse_text(text, &SPEC).map_err(|m| err(src, item, m))?;
            if parsed.status != Status::Deferred {
                return Err(err(
                    src,
                    item,
                    "backlog items are deferred — no other status is allowed",
                ));
            }
            section.items.push(parsed);
        }
    }
    Ok(())
}

/// Canonical `BACKLOG.md` — same layout as [`crate::emit_todo`]
/// minus the `[ ]` checkboxes. Fails on non-deferred items and the
/// same line-grammar limits as TODO.md.
pub fn emit_backlog(doc: &BacklogDoc) -> Result<String, EmitError> {
    let err = |m: &str| EmitError::new(FILE, m);
    let mut out = String::from("# Backlog\n");
    for (i, section) in doc.sections.iter().enumerate() {
        for item in &section.items {
            line::check_item(item, FILE)?;
            if item.status != Status::Deferred {
                return Err(EmitError::new(
                    FILE,
                    format!("non-deferred item {} in BACKLOG.md", item.id),
                ));
            }
        }
        out.push('\n');
        match &section.name {
            Some(name) => {
                if name.trim() != *name || name.is_empty() {
                    return Err(err("empty or untrimmed section name"));
                }
                out.push_str("## ");
                out.push_str(&mdx::inline(name));
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
            out.push_str("- ");
            out.push_str(&mdx::inline(&line::text(item, &SPEC)?));
            out.push('\n');
        }
    }
    Ok(out)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use myco_core::{Item, ItemId};

    #[test]
    fn roundtrip() {
        let src = "# Backlog\n\n## Someday\n\n- T-034 Old idea · pri:low · #maybe\n- T-040 Another\n\n## Rejected\n\n- T-050 Meh\n";
        let doc = parse_backlog(src).unwrap();
        assert_eq!(doc.sections.len(), 2);
        assert_eq!(doc.items().count(), 3);
        assert!(doc.items().all(|i| i.status == Status::Deferred));
        assert_eq!(emit_backlog(&doc).unwrap(), src);
    }

    #[test]
    fn unsectioned() {
        let src = "# Backlog\n\n- T-001 loose\n";
        let doc = parse_backlog(src).unwrap();
        assert_eq!(doc.sections[0].name, None);
        assert_eq!(emit_backlog(&doc).unwrap(), src);
        assert_eq!(parse_backlog(&emit_backlog(&doc).unwrap()).unwrap(), doc);
    }

    #[test]
    fn explicit_deferred_ok() {
        let src = "# Backlog\n\n- T-001 x · status:deferred\n";
        let doc = parse_backlog(src).unwrap();
        assert_eq!(doc.sections[0].items[0].status, Status::Deferred);
        // canonical emit drops the redundant token
        assert_eq!(emit_backlog(&doc).unwrap(), "# Backlog\n\n- T-001 x\n");
    }

    #[test]
    fn empty_doc() {
        let doc = BacklogDoc::default();
        let out = emit_backlog(&doc).unwrap();
        assert_eq!(out, "# Backlog\n");
        assert_eq!(parse_backlog("# Backlog\n").unwrap(), doc);
    }

    #[test]
    fn parse_errors() {
        for (src, why) in [
            ("# Back\n", "bad title"),
            ("## Backlog\n", "h2 title"),
            ("text\n\n# Backlog\n", "not first"),
            ("# Backlog\n\ntext\n", "stray para"),
            ("# Backlog\n\n### x\n", "h3"),
            ("# Backlog\n\n## s *em*\n", "rich heading"),
            ("# Backlog\n\n- [ ] T-001 x\n", "checkbox"),
            ("# Backlog\n\n- T-001 x · status:open\n", "open status"),
            ("# Backlog\n\n- T-001 x · status:done\n", "done status"),
            ("# Backlog\n\n- I-001 x\n", "wrong kind"),
            ("# Backlog\n\n- T-001\n  cont\n", "multi-line"),
            ("# Backlog\n\n1. T-001 x\n", "ordered"),
        ] {
            assert!(parse_backlog(src).is_err(), "{why}: {src:?}");
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
    fn nested_list_is_single_line_error() {
        let src = "# Backlog\n\n- T-001 a\n  - nested\n";
        let e = parse_backlog(src).unwrap_err();
        assert!(e.message().contains("single line"));
    }

    #[test]
    fn parse_list_skips_non_listitem_children() {
        let list = markdown::mdast::List {
            ordered: false,
            start: None,
            spread: false,
            children: vec![Node::Paragraph(markdown::mdast::Paragraph {
                children: vec![],
                position: None,
            })],
            position: None,
        };
        let mut section = Section::default();
        parse_list("", &list, &mut section).unwrap();
        assert!(section.items.is_empty());
    }

    #[test]
    fn emit_errors() {
        let mut doc = BacklogDoc::default();
        doc.sections.push(Section {
            name: Some("s".into()),
            items: vec![Item::new(ItemId::new(ItemKind::Task, 1), "x").status(Status::Open)],
        });
        assert!(emit_backlog(&doc).is_err(), "non-deferred");

        let mut doc = BacklogDoc::default();
        doc.sections.push(Section::named("a"));
        doc.sections.push(Section::unsectioned());
        assert!(emit_backlog(&doc).is_err(), "unsectioned at idx>0");

        let mut doc = BacklogDoc::default();
        doc.sections.push(Section::unsectioned());
        assert!(emit_backlog(&doc).is_err(), "empty unsectioned");

        let mut doc = BacklogDoc::default();
        doc.sections.push(Section {
            name: Some(" x".into()),
            items: vec![],
        });
        assert!(emit_backlog(&doc).is_err(), "untrimmed name");
    }
}
