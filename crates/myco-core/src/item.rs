//! The work item — shared shape for TODO/backlog line items and
//! issue/proposal/adr/plan file metadata (`ItemFile` adds a body).

use std::collections::BTreeSet;

use crate::{Actor, ItemId, Priority, Status};

/// A work item's metadata.
///
/// `created` is `Option<Date>` — the spec grammar
/// (`- [ ] T-012 title · pri:high · #tag`) has no date slot, so a
/// missing `created` must stay representable; `myco check` can flag
/// it later. Emit always writes `created:` when present.
///
/// `note` is reserved for handoffs (`myco handoff --note`); kept in
/// the model now so phase 6 doesn't re-shape it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Item {
    /// Typed id — determines kind, sort, filename.
    pub id: ItemId,
    /// One-line summary. No `·`, no newlines in line-grammar
    /// contexts (enforced at emit, not here — `Item` is a value,
    /// not a validator).
    pub title: String,
    /// Lifecycle state — transitions are validated by the caller
    /// via [`Status::transition_to`].
    pub status: Status,
    /// `pri:` — defaults to [`Priority::Medium`].
    #[serde(default)]
    pub priority: Priority,
    /// `owner:` — `devin`, `human:luis`, …
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schema", schemars(with = "Option<String>"))]
    pub owner: Option<Actor>,
    /// `#label`s — a sorted set, duplicates impossible.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub labels: BTreeSet<String>,
    /// `blocks:` — ids this item blocks. Sorted set.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub blocks: BTreeSet<ItemId>,
    /// `created:` — `YYYY-MM-DD` when known.
    #[serde(default, with = "crate::date::opt")]
    #[cfg_attr(feature = "schema", schemars(with = "Option<String>"))]
    pub created: Option<time::Date>,
    /// Free-form note — handoff/resume payload (phase 6).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// `superseded_by:` — the item that replaced this one (ADRs).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub superseded_by: Option<ItemId>,
    /// `supersedes:` — the item this one replaces (ADRs).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supersedes: Option<ItemId>,
    /// `epic:` — the epic (named delivery grouping) this item
    /// belongs to. Line token `epic:<name>`; a group name, not a
    /// label — one per item.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub epic: Option<String>,
    /// `milestone:` — the milestone this item targets. Line token
    /// `ms:<name>` (kept short — lines stay readable).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub milestone: Option<String>,
}

/// The metadata half of an [`crate::ItemFile`] — identical shape to
/// [`Item`]; the alias exists so signatures read as "frontmatter,
/// not body".
pub type ItemMeta = Item;

impl Item {
    /// Minimal item: id + title, open, medium, nothing else.
    pub fn new(id: ItemId, title: impl Into<String>) -> Self {
        Item {
            id,
            title: title.into(),
            status: Status::Open,
            priority: Priority::Medium,
            owner: None,
            labels: BTreeSet::new(),
            blocks: BTreeSet::new(),
            created: None,
            note: None,
            superseded_by: None,
            supersedes: None,
            epic: None,
            milestone: None,
        }
    }

    /// Builder-style `status` setter (no transition validation —
    /// use [`Status::transition_to`] when advancing a real item).
    pub fn status(mut self, status: Status) -> Self {
        self.status = status;
        self
    }

    /// Builder-style `priority` setter.
    pub fn priority(mut self, p: Priority) -> Self {
        self.priority = p;
        self
    }

    /// Builder-style `owner` setter.
    pub fn owner(mut self, owner: Actor) -> Self {
        self.owner = Some(owner);
        self
    }

    /// Builder-style `created` setter.
    pub fn created(mut self, d: time::Date) -> Self {
        self.created = Some(d);
        self
    }

    /// Builder-style `note` setter.
    pub fn note(mut self, note: impl Into<String>) -> Self {
        self.note = Some(note.into());
        self
    }

    /// Add a `#label`.
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.labels.insert(label.into());
        self
    }

    /// Add a `blocks:` dep.
    pub fn blocks(mut self, id: ItemId) -> Self {
        self.blocks.insert(id);
        self
    }

    /// Builder-style `superseded_by` setter (ADRs).
    pub fn superseded_by(mut self, id: ItemId) -> Self {
        self.superseded_by = Some(id);
        self
    }

    /// Builder-style `supersedes` setter (ADRs).
    pub fn supersedes(mut self, id: ItemId) -> Self {
        self.supersedes = Some(id);
        self
    }

    /// Builder-style `epic` setter.
    pub fn epic(mut self, name: impl Into<String>) -> Self {
        self.epic = Some(name.into());
        self
    }

    /// Builder-style `milestone` setter.
    pub fn milestone(mut self, name: impl Into<String>) -> Self {
        self.milestone = Some(name.into());
        self
    }
}

/// A `#label` is `[A-Za-z0-9_-]+` — kept permissive on the model,
/// enforced at the file-grammar boundary.
pub fn is_valid_label(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
}

/// An `epic:`/`ms:` group name — `[A-Za-z0-9_.-]+`: the label
/// charset plus `.` for `1.0`-style milestones. Same boundary rule
/// as labels (the name must fit a `·`-separated line token).
pub fn is_valid_group_name(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::{ItemId, ItemKind};

    #[test]
    fn builder() {
        let id = ItemId::new(ItemKind::Task, 12);
        let d = crate::date::parse("2026-02-09").unwrap();
        let item = Item::new(id, "write phase 2")
            .status(Status::Claimed)
            .priority(Priority::High)
            .owner("devin".parse().unwrap())
            .created(d)
            .note("session 7")
            .label("core")
            .label("store")
            .blocks(ItemId::new(ItemKind::Task, 10));
        assert_eq!(item.id, id);
        assert_eq!(item.title, "write phase 2");
        assert_eq!(item.status, Status::Claimed);
        assert_eq!(item.priority, Priority::High);
        assert_eq!(item.owner.as_ref().map(Actor::as_str), Some("devin"));
        assert_eq!(item.created, Some(d));
        assert_eq!(item.note.as_deref(), Some("session 7"));
        assert!(item.labels.contains("core") && item.labels.contains("store"));
        assert!(item.blocks.contains(&ItemId::new(ItemKind::Task, 10)));
    }

    #[test]
    fn labels_and_blocks_are_sorted_sets() {
        let mut item = Item::new(ItemId::new(ItemKind::Task, 1), "x");
        item.labels.insert("b".into());
        item.labels.insert("a".into());
        item.labels.insert("a".into());
        item.blocks.insert(ItemId::new(ItemKind::Task, 3));
        item.blocks.insert(ItemId::new(ItemKind::Task, 2));
        let labels: Vec<&String> = item.labels.iter().collect();
        assert_eq!(labels, ["a", "b"]);
        assert_eq!(item.labels.len(), 2);
        let first = item.blocks.iter().next().unwrap();
        assert_eq!(*first, ItemId::new(ItemKind::Task, 2));
    }

    #[test]
    fn label_validation() {
        for ok in ["a", "core-2", "x_y", "A1"] {
            assert!(is_valid_label(ok), "{ok}");
        }
        for bad in ["", "a b", "a/b", "a·b", "é"] {
            assert!(!is_valid_label(bad), "{bad}");
        }
    }

    #[test]
    fn group_name_validation() {
        for ok in ["v2", "q3-launch", "1.0", "alpha_beta", "A1.x"] {
            assert!(is_valid_group_name(ok), "{ok}");
        }
        for bad in ["", "a b", "a/b", "a·b", "a:b", "é"] {
            assert!(!is_valid_group_name(bad), "{bad}");
        }
    }

    #[test]
    fn serde_skips_defaults() {
        let item = Item::new(ItemId::new(ItemKind::Task, 1), "t");
        let json = serde_json::to_value(&item).unwrap();
        let obj = json.as_object().unwrap();
        assert!(!obj.contains_key("owner"));
        assert!(!obj.contains_key("labels"));
        assert!(!obj.contains_key("blocks"));
        assert!(!obj.contains_key("note"));
        assert!(!obj.contains_key("epic"));
        assert!(!obj.contains_key("milestone"));
        // full item round-trips
        let full = Item::new(ItemId::new(ItemKind::Issue, 2), "t")
            .owner("devin".parse().unwrap())
            .created(crate::date::parse("2026-01-01").unwrap())
            .note("n")
            .label("l")
            .blocks(ItemId::new(ItemKind::Task, 1))
            .superseded_by(ItemId::new(ItemKind::Adr, 2))
            .supersedes(ItemId::new(ItemKind::Adr, 1))
            .epic("v2")
            .milestone("1.0");
        let back: Item = serde_json::from_str(&serde_json::to_string(&full).unwrap()).unwrap();
        assert_eq!(back, full);
    }

    #[cfg(feature = "schema")]
    #[test]
    fn json_schema_compiles() {
        let schema = schemars::schema_for!(Item);
        assert!(serde_json::to_value(&schema).is_ok());
    }
}
