//! The item-line grammar shared by `TODO.md` (`- [ ] T-012 …`) and
//! `BACKLOG.md` (`- T-034 …`):
//!
//! ```text
//! <id> <title> [· status:<s>] [· pri:<p>] [· owner:<a>]
//!     [· created:<date>] [· #<label>]… [· blocks:<id>[,<id>]…]
//!     [· links:<id>[,<id>]…] [· epic:<name>] [· ms:<name>]
//!     [· due:<date>] [· est:<n>] [· cycle:<name>] [· note:<text>]
//! ```
//!
//! Metadata tokens come after the title, `·`-separated, in the
//! canonical order above. Titles may not contain `·`. Defaults:
//! `status` = the doc's default (`open` for TODO, `deferred` for
//! BACKLOG), `pri` = `med`. Unknown/malformed tokens are errors —
//! `myco check` should catch typos, not swallow them.
//!
//! Items nest: a `-`/`[ ]` list indented under an item is its
//! `children` (canonical emit = 2 spaces per level; arbitrary
//! depth up to [`MAX_ITEM_DEPTH`], one level recommended). Child
//! lines carry the same grammar — full items, not prose.

use markdown::mdast::Node;
use myco_core::{
    date, is_valid_group_name, is_valid_label, Actor, Item, ItemId, ItemKind, Priority, Status,
    MAX_ESTIMATE,
};

use crate::error::EmitError;

/// Deepest item nesting the docs accept — the spec says arbitrary
/// depth, but parse and emit walk the tree recursively, so past
/// this bound it's an error, not a stack overflow. 64 is far past
/// any real `TODO.md`/`BACKLOG.md` nesting.
pub(crate) const MAX_ITEM_DEPTH: usize = 64;

/// Per-document defaults for line parse/emit.
pub(crate) struct LineSpec {
    /// Kind ids must have (tasks only — `T-NNN`).
    pub kind: ItemKind,
    /// Status when no `status:` token is present.
    pub default_status: Status,
    /// Which file we're in — for emit errors.
    pub file: &'static str,
}

fn bad(msg: impl Into<String>) -> String {
    msg.into()
}

/// A list item's shape: its `Paragraph` head line plus at most one
/// nested unordered `List` (the item's children). Anything else —
/// a second paragraph, an ordered list, a second nested list —
/// breaks the grammar.
pub(crate) fn item_parts(
    li: &markdown::mdast::ListItem,
) -> Result<(&Node, Option<&markdown::mdast::List>), String> {
    let mut children = li.children.iter();
    let Some(head @ Node::Paragraph(_)) = children.next() else {
        return Err(bad("item must be a single line"));
    };
    let mut nested = None;
    for c in children {
        match c {
            Node::List(l) if !l.ordered && nested.is_none() => nested = Some(l),
            _ => {
                return Err(bad(
                    "unexpected block under item — only a nested `-` list is allowed",
                ))
            }
        }
    }
    Ok((head, nested))
}

/// Parse the text inside a list item (after `- [ ] ` / `- `).
pub(crate) fn parse_text(text: &str, spec: &LineSpec) -> Result<Item, String> {
    if text.contains('\n') {
        return Err(bad("item must be a single line"));
    }
    let mut parts = text.split('·');
    let head = parts.next().unwrap_or_default().trim();
    let mut head_parts = head.splitn(2, char::is_whitespace);
    let id_str = head_parts
        .next()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| bad("expected item id (T-NNN)"))?;
    let id: ItemId = id_str.parse().map_err(|e| format!("{e}"))?;
    if id.kind() != spec.kind {
        return Err(bad(format!(
            "wrong item kind {id} — {file} holds {} items only",
            spec.kind.prefix(),
            file = spec.file
        )));
    }
    let title = head_parts.next().unwrap_or("").trim().to_string();

    let mut status = None;
    let mut priority = None;
    let mut owner = None;
    let mut created = None;
    let mut blocks_seen = false;
    let mut links_seen = false;
    let mut epic = None;
    let mut milestone = None;
    let mut due = None;
    let mut estimate = None;
    let mut cycle = None;
    let mut note = None;
    let mut item = Item::new(id, title);

    for raw in parts {
        let tok = raw.trim();
        if tok.is_empty() {
            return Err(bad("empty metadata token — double `·` or trailing `·`"));
        }
        if let Some(label) = tok.strip_prefix('#') {
            if !is_valid_label(label) {
                return Err(bad(format!("invalid label #{label:?}")));
            }
            item.labels.insert(label.to_string());
            continue;
        }
        let (key, value) = tok.split_once(':').ok_or_else(|| {
            bad(format!(
                "malformed metadata token {tok:?} — expected key:value or #label"
            ))
        })?;
        let value = value.trim();
        if value.is_empty() {
            return Err(bad(format!("empty value in token {tok:?}")));
        }
        match key {
            "status" => {
                if status.is_some() {
                    return Err(bad("duplicate status: token"));
                }
                status = Some(value.parse::<Status>().map_err(|e| format!("{e}"))?);
            }
            "pri" => {
                if priority.is_some() {
                    return Err(bad("duplicate pri: token"));
                }
                priority = Some(value.parse::<Priority>().map_err(|e| format!("{e}"))?);
            }
            "owner" => {
                if owner.is_some() {
                    return Err(bad("duplicate owner: token"));
                }
                owner = Some(Actor::new(value).map_err(|e| format!("{e}"))?);
            }
            "created" => {
                if created.is_some() {
                    return Err(bad("duplicate created: token"));
                }
                created = Some(date::parse(value).map_err(|e| format!("bad created: date — {e}"))?);
            }
            "blocks" => {
                if blocks_seen {
                    return Err(bad("duplicate blocks: token"));
                }
                blocks_seen = true;
                for bid in value.split(',') {
                    let bid = bid.trim();
                    if bid.is_empty() {
                        return Err(bad("empty id in blocks: list"));
                    }
                    item.blocks
                        .insert(bid.parse::<ItemId>().map_err(|e| format!("{e}"))?);
                }
            }
            "links" => {
                if links_seen {
                    return Err(bad("duplicate links: token"));
                }
                links_seen = true;
                for lid in value.split(',') {
                    let lid = lid.trim();
                    if lid.is_empty() {
                        return Err(bad("empty id in links: list"));
                    }
                    item.links
                        .insert(lid.parse::<ItemId>().map_err(|e| format!("{e}"))?);
                }
            }
            "epic" => {
                if epic.is_some() {
                    return Err(bad("duplicate epic: token"));
                }
                if !is_valid_group_name(value) {
                    return Err(bad(format!("invalid epic name {value:?}")));
                }
                epic = Some(value.to_string());
            }
            "ms" => {
                if milestone.is_some() {
                    return Err(bad("duplicate ms: token"));
                }
                if !is_valid_group_name(value) {
                    return Err(bad(format!("invalid milestone name {value:?}")));
                }
                milestone = Some(value.to_string());
            }
            "due" => {
                if due.is_some() {
                    return Err(bad("duplicate due: token"));
                }
                due = Some(date::parse(value).map_err(|e| format!("bad due: date — {e}"))?);
            }
            "est" => {
                if estimate.is_some() {
                    return Err(bad("duplicate est: token"));
                }
                let n: u32 = value.parse().map_err(|_| {
                    bad(format!("bad est: value {value:?} — expected a point count"))
                })?;
                if n > MAX_ESTIMATE {
                    return Err(bad(format!("est:{n} exceeds max {MAX_ESTIMATE}")));
                }
                estimate = Some(n);
            }
            "cycle" => {
                if cycle.is_some() {
                    return Err(bad("duplicate cycle: token"));
                }
                if !is_valid_group_name(value) {
                    return Err(bad(format!("invalid cycle name {value:?}")));
                }
                cycle = Some(value.to_string());
            }
            "note" => {
                if note.is_some() {
                    return Err(bad("duplicate note: token"));
                }
                note = Some(value.to_string());
            }
            other => {
                return Err(bad(format!(
                    "unknown metadata key {other:?} — expected status|pri|owner|created|blocks|links|epic|ms|due|est|cycle|note or #label"
                )));
            }
        }
    }

    if item.blocks.contains(&id) {
        return Err(bad(format!("{id} can't block itself")));
    }
    if item.links.contains(&id) {
        return Err(bad(format!("{id} can't link itself")));
    }
    item.status = status.unwrap_or(spec.default_status);
    item.priority = priority.unwrap_or_default();
    item.owner = owner;
    item.created = created;
    item.epic = epic;
    item.milestone = milestone;
    item.due = due;
    item.estimate = estimate;
    item.cycle = cycle;
    item.note = note;
    Ok(item)
}

/// Validate what the line grammar can't express. Shared by TODO and
/// BACKLOG emit — the caller adds status rules on top.
pub(crate) fn check_item(item: &Item, file: &'static str) -> Result<(), EmitError> {
    let err = |m: &str| EmitError::new(file, format!("{m} ({})", item.id));
    if item.title != item.title.trim() {
        return Err(err("untrimmed title"));
    }
    if item.title.contains('·') {
        return Err(err("title contains `·`"));
    }
    if item.title.contains('\n') || item.title.contains('\r') {
        return Err(err("title contains a newline"));
    }
    for label in &item.labels {
        if !is_valid_label(label) {
            return Err(err(&format!("invalid label #{label:?}")));
        }
    }
    for (what, name) in [
        ("epic", item.epic.as_deref()),
        ("milestone", item.milestone.as_deref()),
        ("cycle", item.cycle.as_deref()),
    ] {
        if let Some(name) = name {
            if !is_valid_group_name(name) {
                return Err(err(&format!("invalid {what} name {name:?}")));
            }
        }
    }
    if let Some(e) = item.estimate {
        if e > MAX_ESTIMATE {
            return Err(err(&format!("est:{e} exceeds max {MAX_ESTIMATE}")));
        }
    }
    if let Some(note) = &item.note {
        if note.is_empty() || note != note.trim() {
            return Err(err("untrimmed/empty note"));
        }
        if note.contains('·') || note.contains('\n') || note.contains('\r') {
            return Err(err("note contains `·` or a newline"));
        }
    }
    if item.blocks.contains(&item.id)
        || item.links.contains(&item.id)
        || item.supersedes == Some(item.id)
        || item.superseded_by == Some(item.id)
    {
        return Err(err(
            "self-reference — can't block, link, or supersede itself",
        ));
    }
    Ok(())
}

/// The line text (`<id> <title> · <tokens>`) — checkbox/marker is
/// the caller's job.
pub(crate) fn text(item: &Item, spec: &LineSpec) -> Result<String, EmitError> {
    check_item(item, spec.file)?;
    let mut s = item.id.to_string();
    if !item.title.is_empty() {
        s.push(' ');
        s.push_str(&item.title);
    }
    let mut tokens: Vec<String> = Vec::new();
    if item.status != spec.default_status {
        tokens.push(format!("status:{}", item.status.token()));
    }
    if item.priority != Priority::Medium {
        tokens.push(format!("pri:{}", item.priority.token()));
    }
    if let Some(owner) = &item.owner {
        tokens.push(format!("owner:{owner}"));
    }
    if let Some(d) = item.created {
        tokens.push(format!("created:{}", date::render(d)));
    }
    for label in &item.labels {
        tokens.push(format!("#{label}"));
    }
    if !item.blocks.is_empty() {
        let ids: Vec<String> = item.blocks.iter().map(ItemId::to_string).collect();
        tokens.push(format!("blocks:{}", ids.join(",")));
    }
    if !item.links.is_empty() {
        let ids: Vec<String> = item.links.iter().map(ItemId::to_string).collect();
        tokens.push(format!("links:{}", ids.join(",")));
    }
    if let Some(epic) = &item.epic {
        tokens.push(format!("epic:{epic}"));
    }
    if let Some(ms) = &item.milestone {
        tokens.push(format!("ms:{ms}"));
    }
    if let Some(d) = item.due {
        tokens.push(format!("due:{}", date::render(d)));
    }
    if let Some(e) = item.estimate {
        tokens.push(format!("est:{e}"));
    }
    if let Some(c) = &item.cycle {
        tokens.push(format!("cycle:{c}"));
    }
    if let Some(note) = &item.note {
        tokens.push(format!("note:{note}"));
    }
    for t in tokens {
        s.push_str(" · ");
        s.push_str(&t);
    }
    Ok(s)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const TODO: LineSpec = LineSpec {
        kind: ItemKind::Task,
        default_status: Status::Open,
        file: "TODO.md",
    };

    fn item(text: &str) -> Item {
        parse_text(text, &TODO).unwrap()
    }

    #[test]
    fn parse_full_line() {
        let i = item("T-012 Write the thing · status:in-progress · pri:high · owner:devin · created:2026-02-09 · #core · #store · blocks:T-010, I-002 · epic:v2 · ms:1.0 · due:2026-12-01 · est:3 · cycle:sprint-12 · note:from src/x.rs:9");
        assert_eq!(i.id, ItemId::new(ItemKind::Task, 12));
        assert_eq!(i.title, "Write the thing");
        assert_eq!(i.status, Status::InProgress);
        assert_eq!(i.priority, Priority::High);
        assert_eq!(i.owner.as_ref().unwrap().as_str(), "devin");
        assert_eq!(i.created, Some(date::parse("2026-02-09").unwrap()));
        assert!(i.labels.contains("core") && i.labels.contains("store"));
        assert!(i.blocks.contains(&ItemId::new(ItemKind::Task, 10)));
        assert!(i.blocks.contains(&ItemId::new(ItemKind::Issue, 2)));
        assert_eq!(i.epic.as_deref(), Some("v2"));
        assert_eq!(i.milestone.as_deref(), Some("1.0"));
        assert_eq!(i.due, Some(date::parse("2026-12-01").unwrap()));
        assert_eq!(i.estimate, Some(3));
        assert_eq!(i.cycle.as_deref(), Some("sprint-12"));
        assert_eq!(i.note.as_deref(), Some("from src/x.rs:9"));
    }

    #[test]
    fn parse_scheduling_tokens() {
        // scheduling tokens are optional and additive — bare lines
        // parse exactly as before
        let i = item("T-003 plain task");
        assert!(i.due.is_none() && i.estimate.is_none() && i.cycle.is_none());
        let i = item("T-004 x · due:2026-12-01");
        assert_eq!(i.due, Some(date::parse("2026-12-01").unwrap()));
        let i = item("T-005 x · est:0");
        assert_eq!(i.estimate, Some(0));
        // round-trip in canonical order — est:0 still emits
        // (Some(0) is set, not a default)
        for line in [
            "T-006 x · due:2026-12-01 · est:3 · cycle:sprint-12",
            "T-007 x · est:0 · cycle:s-1",
        ] {
            let i = item(line);
            let s = text(&i, &TODO).unwrap();
            assert_eq!(s, line, "{s}");
            assert_eq!(parse_text(&s, &TODO).unwrap(), i);
        }
    }

    #[test]
    fn parse_links_token() {
        let i = item("T-012 task · links:PL-001, P-013 · blocks:T-010");
        assert!(i.links.contains(&ItemId::new(ItemKind::Plan, 1)));
        assert!(i.links.contains(&ItemId::new(ItemKind::Proposal, 13)));
        assert_eq!(i.links.len(), 2);
        // emit order: links sits between blocks and epic, ids sorted
        let s = text(&i, &TODO).unwrap();
        assert!(s.contains("blocks:T-010 · links:P-013,PL-001"), "{s}");
        assert_eq!(parse_text(&s, &TODO).unwrap(), i);
    }

    #[test]
    fn parse_minimal() {
        let i = item("T-003 title only");
        assert_eq!(i.status, Status::Open);
        assert_eq!(i.priority, Priority::Medium);
        assert!(i.owner.is_none() && i.created.is_none() && i.note.is_none());
        let bare = item("T-004");
        assert_eq!(bare.title, "");
    }

    #[test]
    fn parse_errors() {
        for (text, why) in [
            ("x", "bad id"),
            ("I-001 issue here", "wrong kind"),
            ("", "empty head"),
            ("T-001 t · bogus:1", "unknown key"),
            ("T-001 t · orphan", "no key"),
            ("T-001 t · status:", "empty value"),
            ("T-001 t · status:wip", "bad status"),
            ("T-001 t · status:open · status:claimed", "dup status"),
            ("T-001 t · pri:max", "bad pri"),
            ("T-001 t · pri:low · pri:high", "dup pri"),
            ("T-001 t · owner:has space", "bad actor"),
            ("T-001 t · owner:a · owner:b", "dup owner"),
            ("T-001 t · created:tomorrow", "bad date"),
            (
                "T-001 t · created:2026-01-01 · created:2026-01-02",
                "dup created",
            ),
            ("T-001 t · blocks:", "empty blocks"),
            ("T-001 t · blocks:T-1,,T-2", "empty id"),
            ("T-001 t · blocks:XYZ", "bad block id"),
            ("T-001 t · blocks:T-1 · blocks:T-2", "dup blocks"),
            ("T-001 t · blocks:T-1,T-2", "blocks itself"),
            ("T-001 t · links:", "empty links"),
            ("T-001 t · links:T-1,,T-2", "empty id in links"),
            ("T-001 t · links:XYZ", "bad link id"),
            ("T-001 t · links:T-1 · links:T-2", "dup links"),
            ("T-001 t · links:T-001", "links itself"),
            ("T-001 t · epic:", "empty epic"),
            ("T-001 t · epic:has space", "bad epic name"),
            ("T-001 t · epic:a · epic:b", "dup epic"),
            ("T-001 t · ms:", "empty ms"),
            ("T-001 t · ms:has space", "bad ms name"),
            ("T-001 t · ms:a · ms:b", "dup ms"),
            ("T-001 t · due:", "empty due"),
            ("T-001 t · due:tomorrow", "bad due date"),
            ("T-001 t · due:2026-13-40", "bad due date"),
            ("T-001 t · due:2026-01-01 · due:2026-01-02", "dup due"),
            ("T-001 t · est:", "empty est"),
            ("T-001 t · est:three", "non-numeric est"),
            ("T-001 t · est:-1", "negative est"),
            ("T-001 t · est:1.5", "fractional est"),
            ("T-001 t · est:1001", "est over max"),
            ("T-001 t · est:1 · est:2", "dup est"),
            ("T-001 t · cycle:", "empty cycle"),
            ("T-001 t · cycle:has space", "bad cycle name"),
            ("T-001 t · cycle:a · cycle:b", "dup cycle"),
            ("T-001 t · note:", "empty note"),
            ("T-001 t · note:a · note:b", "dup note"),
            ("T-001 t · #", "empty label"),
            ("T-001 t · #a b", "bad label char"),
            ("T-001 t · #bad/lab", "bad label char"),
            ("T-001 t · ", "trailing dot"),
            ("T-001 t ·· status:open", "double dot"),
        ] {
            assert!(parse_text(text, &TODO).is_err(), "{why}: {text:?}");
        }
    }

    #[test]
    fn emit_canonical_order() {
        let mut i = Item::new(ItemId::new(ItemKind::Task, 12), "Write the thing")
            .status(Status::InProgress)
            .priority(Priority::High)
            .owner(Actor::new("devin").unwrap())
            .created(date::parse("2026-02-09").unwrap())
            .label("store")
            .label("core")
            .blocks(ItemId::new(ItemKind::Issue, 2))
            .blocks(ItemId::new(ItemKind::Task, 10))
            .epic("v2")
            .milestone("1.0")
            .due(date::parse("2026-12-01").unwrap())
            .estimate(3)
            .cycle("sprint-12")
            .note("swept from src/x.rs:9");
        let s = text(&i, &TODO).unwrap();
        assert_eq!(
            s,
            "T-012 Write the thing · status:in-progress · pri:high · owner:devin · created:2026-02-09 · #core · #store · blocks:T-010,I-002 · epic:v2 · ms:1.0 · due:2026-12-01 · est:3 · cycle:sprint-12 · note:swept from src/x.rs:9"
        );
        // round trip
        assert_eq!(parse_text(&s, &TODO).unwrap(), i);
        // defaults emit nothing
        i.status = Status::Open;
        i.priority = Priority::Medium;
        let s2 = text(&i, &TODO).unwrap();
        assert!(!s2.contains("status:") && !s2.contains("pri:"));
        assert_eq!(parse_text(&s2, &TODO).unwrap(), i);
    }

    #[test]
    fn emit_rejects() {
        let mut i = Item::new(ItemId::new(ItemKind::Task, 1), " ok ");
        assert!(text(&i, &TODO).is_err());
        i.title = "has · dot".into();
        assert!(text(&i, &TODO).is_err());
        i.title = "two\nlines".into();
        assert!(text(&i, &TODO).is_err());
        i.title = "fine".into();
        i.labels.insert("bad label".into());
        assert!(text(&i, &TODO).is_err());
        i.labels.clear();
        i.epic = Some("bad epic".into());
        assert!(text(&i, &TODO).is_err());
        i.epic = None;
        i.milestone = Some("bad/ms".into());
        assert!(text(&i, &TODO).is_err());
        i.milestone = None;
        i.cycle = Some("bad/cycle".into());
        assert!(text(&i, &TODO).is_err());
        i.cycle = None;
        i.estimate = Some(MAX_ESTIMATE + 1);
        assert!(text(&i, &TODO).is_err());
        i.estimate = None;
        i.note = Some("has · dot".into());
        assert!(text(&i, &TODO).is_err());
        i.note = Some(" trailing ".into());
        assert!(text(&i, &TODO).is_err());
        i.note = Some("two\nlines".into());
        assert!(text(&i, &TODO).is_err());
        // self-references — an item can't block or supersede
        // itself, however the model got built (I-021)
        i.note = None;
        i.blocks.insert(ItemId::new(ItemKind::Task, 1));
        assert!(text(&i, &TODO).is_err());
        i.blocks.clear();
        i.supersedes = Some(ItemId::new(ItemKind::Task, 1));
        assert!(text(&i, &TODO).is_err());
        i.supersedes = None;
        i.superseded_by = Some(ItemId::new(ItemKind::Task, 1));
        assert!(text(&i, &TODO).is_err());
    }
}
