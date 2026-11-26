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

/// Is `line` a terminal item? `[x]`/`[X]` checkbox, or a
/// `· status:done`/`· status:cancelled` metadata token — both
/// archive into DONE.md, so neither belongs in TODO.md/BACKLOG.md.
/// Shared by `myco check`'s done-item finding and
/// [`strip_terminal`]'s removal pass — the two must agree on what
/// counts as terminal or the gate and the fixer disagree.
pub fn is_done_line(line: &str) -> bool {
    let t = line.trim_start();
    if ["- [x]", "- [X]", "* [x]", "* [X]", "+ [x]", "+ [X]"]
        .iter()
        .any(|m| t.starts_with(m))
    {
        return true;
    }
    t.split('·')
        .skip(1)
        .any(|p| matches!(p.trim(), "status:done" | "status:cancelled"))
}

/// The `LineSpec` terminal extraction parses with — tasks, and the
/// default is moot: every extracted item is normalized to the
/// terminal pair (`done`/`cancelled`) before it can reach DONE.md.
const TASK_SPEC: LineSpec = LineSpec {
    kind: ItemKind::Task,
    default_status: Status::Open,
    file: FILE,
};

/// What [`strip_terminal`] produced: the source minus the terminal
/// item blocks, plus the items those blocks parsed into (subtrees
/// included — every node forced to the terminal pair, mirroring
/// `todo done`/`todo cancel`'s whole-subtree archive).
#[derive(Debug, Default)]
pub struct StripOutcome {
    /// The input with every *archivable* terminal block removed.
    /// Terminal lines that don't parse into an item stay — they
    /// can't be archived, so they can't be silently dropped either.
    pub src: String,
    /// The removed items, in file order, ready for [`crate::emit_done`].
    pub items: Vec<Item>,
}

/// The text inside a `-`/`*`/`+` bullet — checkbox (`[ ]`/`[x]`/
/// `[X]`) stripped — or `None` when `t` isn't a bullet at all.
/// `Some("")` is a bare bullet (no item text).
fn bullet_text(t: &str) -> Option<&str> {
    let mut rest = t.strip_prefix(['-', '*', '+'])?.trim_start();
    for cb in ["[ ]", "[x]", "[X]"] {
        if let Some(r) = rest.strip_prefix(cb) {
            // the checkbox must be followed by a boundary —
            // `- [x]y` is text, not a marker
            if r.is_empty() || r.starts_with(char::is_whitespace) {
                rest = r.trim_start();
                break;
            }
        }
    }
    Some(rest)
}

/// Parse one terminal block — the done line plus its deeper-
/// indented subtree — into an [`Item`]. `None` when any item-
/// shaped line fails the line grammar: the caller then keeps the
/// whole block rather than archive a partial tree and drop the
/// rest. Non-bullet deeper lines (continuations) carry no item
/// and drop with the block, as they always have.
fn parse_done_block(block: &[&str]) -> Option<Item> {
    // the done line itself is the archive root — it must parse;
    // a terminal marker that isn't an item (`foo · status:done`
    // prose) keeps the whole block rather than invent a root
    let first = block.first()?;
    let root_text = bullet_text(first.trim()).filter(|s| !s.is_empty())?;
    let at = first.len() - first.trim_start().len();
    let root = line::parse_text(root_text, &TASK_SPEC).ok()?;
    // (indent, item) per bullet line in the rest of the block —
    // an item-shaped line that fails the grammar aborts the whole
    // block: archiving the parseable part would drop the rest
    let mut entries: Vec<(usize, Item)> = vec![(at, root)];
    for l in &block[1..] {
        let t = l.trim();
        let Some(text) = bullet_text(t) else {
            continue; // not a bullet — continuation/prose, not an item
        };
        if text.is_empty() {
            continue; // bare bullet — no item to keep or lose
        }
        let indent = l.len() - l.trim_start().len();
        entries.push((indent, line::parse_text(text, &TASK_SPEC).ok()?));
    }
    // parent[j] = index of the nearest preceding entry with a
    // strictly smaller indent — the subtree grammar
    let n = entries.len();
    let mut parent = vec![usize::MAX; n];
    let mut stack: Vec<usize> = Vec::new();
    for j in 0..n {
        while stack.last().is_some_and(|&t| entries[t].0 >= entries[j].0) {
            stack.pop();
        }
        parent[j] = stack.last().copied().unwrap_or(usize::MAX);
        stack.push(j);
    }
    // children adjacency, then a bounded recursive build
    let mut kids: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (j, &p) in parent.iter().enumerate().skip(1) {
        if p != usize::MAX {
            kids[p].push(j);
        }
    }
    fn build(
        j: usize,
        entries: &[(usize, Item)],
        kids: &[Vec<usize>],
        depth: usize,
    ) -> Option<Item> {
        if depth > line::MAX_ITEM_DEPTH {
            return None;
        }
        let mut item = entries[j].1.clone();
        for &c in &kids[j] {
            item.children.push(build(c, entries, kids, depth + 1)?);
        }
        Some(item)
    }
    let mut root = build(0, &entries, &kids, 0)?;
    // normalize to the terminal pair: `status:cancelled` keeps its
    // token (the archive records *why* it left); everything else —
    // `[x]` with no token, `status:done`, even a stale live status
    // on a checked line — archives as done
    let term = match root.status {
        Status::Cancelled => Status::Cancelled,
        _ => Status::Done,
    };
    // the whole subtree takes the root's terminal status — the
    // same all-or-nothing `todo done`/`todo cancel` apply
    let mut stack = vec![&mut root];
    while let Some(i) = stack.pop() {
        i.status = term;
        stack.extend(i.children.iter_mut());
    }
    Some(root)
}

/// Remove terminal item blocks (`[x]` checkboxes, `status:done`/
/// `status:cancelled` tokens) from TODO.md/BACKLOG.md source and
/// return the cleaned text plus the items to archive into DONE.md.
///
/// This is the archiving half of "terminal items leave the ledger
/// docs": every removed *item* comes back parsed so the caller can
/// push it through `done_push` (which stamps `done:`). A terminal
/// line that doesn't parse — no `T-NNN` id, a malformed token, a
/// child that breaks the grammar — keeps its whole block in the
/// output: `check` keeps reporting it rather than drop it silently.
pub fn strip_terminal(src: &str) -> StripOutcome {
    let mut out = String::with_capacity(src.len());
    let mut items = Vec::new();
    let lines: Vec<&str> = src.split_inclusive('\n').collect();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if !is_done_line(line) {
            out.push_str(line);
            i += 1;
            continue;
        }
        // the block a done line takes with it: itself plus every
        // deeper-indented line (same rule the old textual strip
        // used — a same-or-shallower indent ends it)
        let at = line.len() - line.trim_start().len();
        let mut j = i + 1;
        while j < lines.len() {
            let l = lines[j];
            let indented = l.starts_with(' ') || l.starts_with('\t');
            let indent = l.len() - l.trim_start().len();
            if indented && indent > at {
                j += 1;
            } else {
                break;
            }
        }
        match parse_done_block(&lines[i..j]) {
            Some(item) => items.push(item),
            // unarchivable — leave every line so nothing is lost
            None => out.extend(lines[i..j].iter().copied()),
        }
        i = j;
    }
    StripOutcome { src: out, items }
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
    fn strip_terminal_archives_items() {
        // the I-005 regression: terminal lines used to be deleted
        // outright — now they come back as items for DONE.md
        let src = "# TODO — x\n\n- [ ] T-001 live\n- [x] T-002 shipped\n- [ ] T-003 cancelled · status:cancelled\n  - [ ] T-004 sub\n- [ ] T-005 drop · status:done\n";
        let out = strip_terminal(src);
        assert_eq!(out.src, "# TODO — x\n\n- [ ] T-001 live\n");
        assert_eq!(out.items.len(), 3);
        assert_eq!(out.items[0].id.number(), 2);
        assert_eq!(out.items[0].status, Status::Done);
        assert_eq!(out.items[1].id.number(), 3);
        assert_eq!(out.items[1].status, Status::Cancelled);
        // the subtree archives with the parent, forced terminal
        assert_eq!(out.items[1].children.len(), 1);
        assert_eq!(out.items[1].children[0].status, Status::Cancelled);
        assert_eq!(out.items[2].status, Status::Done);
        // the archive-ready items emit cleanly
        let mut doc = DoneDoc::default();
        doc.sections.push(Section::unsectioned());
        for i in out.items {
            doc.sections[0].items.push(i);
        }
        emit_done(&doc).unwrap();
    }

    #[test]
    fn strip_terminal_keeps_unarchivable_lines() {
        // no T-NNN id → can't archive → the line stays put and
        // `check` keeps reporting it (never a silent drop)
        let src = "- [x] buy milk\n- [ ] T-001 live\n";
        let out = strip_terminal(src);
        assert_eq!(out.src, src);
        assert!(out.items.is_empty());
        // an unparseable *child* blocks the whole done item too
        let src = "- [x] T-001 done\n  - [ ] not-an-id child\n- [ ] T-002 live\n";
        let out = strip_terminal(src);
        assert_eq!(out.src, src);
        assert!(out.items.is_empty());
        // a done *child* of a live parent archives standalone
        let src = "- [ ] T-001 parent\n  - [x] T-002 done kid\n  - [ ] T-003 sib\n";
        let out = strip_terminal(src);
        assert_eq!(out.src, "- [ ] T-001 parent\n  - [ ] T-003 sib\n");
        assert_eq!(out.items.len(), 1);
        assert_eq!(out.items[0].id.number(), 2);
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
