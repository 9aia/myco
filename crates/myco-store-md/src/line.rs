//! The item-line grammar shared by `TODO.md` (`- [ ] T-012 …`) and
//! `BACKLOG.md` (`- T-034 …`):
//!
//! ```text
//! <id> <title> [· status:<s>] [· pri:<p>] [· owner:<a>]
//!     [· created:<date>] [· #<label>]… [· blocks:<id>[,<id>]…]
//! ```
//!
//! Metadata tokens come after the title, `·`-separated, in the
//! canonical order above. Titles may not contain `·`. Defaults:
//! `status` = the doc's default (`open` for TODO, `deferred` for
//! BACKLOG), `pri` = `med`. Unknown/malformed tokens are errors —
//! `myco check` should catch typos, not swallow them.

use myco_core::{date, is_valid_label, Actor, Item, ItemId, ItemKind, Priority, Status};

use crate::error::EmitError;

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
            other => {
                return Err(bad(format!(
                    "unknown metadata key {other:?} — expected status|pri|owner|created|blocks or #label"
                )));
            }
        }
    }

    item.status = status.unwrap_or(spec.default_status);
    item.priority = priority.unwrap_or_default();
    item.owner = owner;
    item.created = created;
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
        let i = item("T-012 Write the thing · status:in-progress · pri:high · owner:devin · created:2026-02-09 · #core · #store · blocks:T-010, I-002");
        assert_eq!(i.id, ItemId::new(ItemKind::Task, 12));
        assert_eq!(i.title, "Write the thing");
        assert_eq!(i.status, Status::InProgress);
        assert_eq!(i.priority, Priority::High);
        assert_eq!(i.owner.as_ref().unwrap().as_str(), "devin");
        assert_eq!(i.created, Some(date::parse("2026-02-09").unwrap()));
        assert!(i.labels.contains("core") && i.labels.contains("store"));
        assert!(i.blocks.contains(&ItemId::new(ItemKind::Task, 10)));
        assert!(i.blocks.contains(&ItemId::new(ItemKind::Issue, 2)));
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
            .blocks(ItemId::new(ItemKind::Task, 10));
        let s = text(&i, &TODO).unwrap();
        assert_eq!(
            s,
            "T-012 Write the thing · status:in-progress · pri:high · owner:devin · created:2026-02-09 · #core · #store · blocks:T-010,I-002"
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
    }
}
