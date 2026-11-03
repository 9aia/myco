//! Typed item ids in one namespace — `T-012` tasks, `I-003` issues,
//! `P-002` proposals, `ADR-0004` ADRs, `PL-001` plans, plus user
//! kinds declared via `[[kinds]]` (P-008).
//!
//! [`ItemKind`] is the *id identity* — the prefix the id grammar
//! carries (`T`, `ADR`, a user's `S`). It's `Copy` data, not a
//! closed enum, so `ItemId` parsing/serde needs no kind table:
//! any `[A-Z][A-Z0-9]*` prefix is grammatically valid. Whether a
//! prefix is *declared* — and the metadata behind it (name,
//! storage, states) — lives in [`crate::KindTable`]; `check` and
//! the store resolve through it.
//!
//! Ordering is declaration order for the five builtin kinds, then
//! prefix-alphabetical for user kinds — a pure function of the
//! prefix, so `BTreeSet<ItemId>` stays consistent without a table.

use std::fmt;
use std::str::FromStr;

/// Max prefix length — keeps `ItemKind` (and so `ItemId`) `Copy`.
const MAX_PREFIX: usize = 15;

/// Inline ASCII prefix — `T`, `ADR`, `S`.
#[derive(Clone, Copy)]
struct Prefix {
    buf: [u8; MAX_PREFIX],
    len: u8,
}

impl Prefix {
    /// Compile-time constructor for the builtin consts.
    const fn build(s: &str) -> Prefix {
        let b = s.as_bytes();
        let mut buf = [0u8; MAX_PREFIX];
        let mut i = 0;
        while i < b.len() {
            buf[i] = b[i];
            i += 1;
        }
        Prefix {
            buf,
            len: b.len() as u8,
        }
    }

    fn as_str(&self) -> &str {
        std::str::from_utf8(&self.buf[..self.len as usize]).unwrap_or("")
    }
}

/// Is `s` a legal id prefix — `[A-Z][A-Z0-9]*`, ≤ [`MAX_PREFIX`]?
fn valid_prefix(s: &str) -> bool {
    let mut it = s.bytes();
    matches!(it.next(), Some(b) if b.is_ascii_uppercase())
        && s.len() <= MAX_PREFIX
        && it.all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
}

/// The kind of work item an [`ItemId`] addresses — data since
/// P-008: the prefix is the identity, `pad` is the canonical
/// emit width (`T-007` → 3, `ADR-0004` → 4). The five builtins
/// exist as associated consts — `ItemKind::Task` and friends
/// keep working — and [`crate::KindSpec`] carries the rest of
/// what used to be enum semantics.
///
/// `PartialEq`/`Hash` compare the **prefix only** — `pad` is a
/// display hint, so `S-0007` (spec pad 4) and a hand-typed `S-7`
/// (pad hint 3) are the same id. `Ord` is builtin-declaration
/// order, then prefix-alphabetical for user kinds.
#[derive(Clone, Copy)]
pub struct ItemKind {
    prefix: Prefix,
    /// Canonical zero-pad for emitted ids — a hint, ignored by
    /// `PartialEq`/`Ord`/`Hash`.
    pad: u8,
}

#[allow(non_upper_case_globals)] // enum-variant-style consts — `ItemKind::Task` is the established spelling
impl ItemKind {
    /// `T-NNN` — tasks live in `TODO.md` / `BACKLOG.md`, no file.
    pub const Task: ItemKind = ItemKind::fixed(Prefix::build("T"), 3);
    /// `I-NNN` — one file each under `issues/`.
    pub const Issue: ItemKind = ItemKind::fixed(Prefix::build("I"), 3);
    /// `P-NNN` — one file each under `proposals/`.
    pub const Proposal: ItemKind = ItemKind::fixed(Prefix::build("P"), 3);
    /// `ADR-NNNN` — one file each under `adrs/`.
    pub const Adr: ItemKind = ItemKind::fixed(Prefix::build("ADR"), 4);
    /// `PL-NNN` — one file each under `plans/`.
    pub const Plan: ItemKind = ItemKind::fixed(Prefix::build("PL"), 3);

    const fn fixed(prefix: Prefix, pad: u8) -> ItemKind {
        ItemKind { prefix, pad }
    }

    /// The builtin kinds, in declaration order. User kinds have
    /// no `ItemKind` const — resolve them through
    /// [`crate::KindTable::resolve`]/[`crate::KindSpec::id_kind`].
    pub const ALL: [ItemKind; 5] = [
        ItemKind::Task,
        ItemKind::Issue,
        ItemKind::Proposal,
        ItemKind::Adr,
        ItemKind::Plan,
    ];

    /// Id prefix — `T`, `I`, `P`, `ADR`, `PL`, or a user prefix.
    pub fn prefix(&self) -> &str {
        self.prefix.as_str()
    }

    /// Canonical zero-pad width in the emitted id form.
    pub fn width(&self) -> usize {
        self.pad as usize
    }

    /// `meta.toml` `[next]` key — lowercase prefix (`t`, `adr`).
    pub fn counter_key(&self) -> String {
        self.prefix().to_lowercase()
    }

    /// Kind for `prefix` — grammatically valid means accepted;
    /// *declared* is a [`crate::KindTable`] question. `pad` 0
    /// picks the builtin width, else 3.
    pub fn from_prefix(prefix: &str) -> Option<ItemKind> {
        ItemKind::for_spec(prefix, 0)
    }

    /// Kind for `prefix` with an explicit emit width — how a
    /// [`crate::KindSpec`] (`zero_pad`) mints its identity.
    pub fn for_spec(prefix: &str, pad: usize) -> Option<ItemKind> {
        if !valid_prefix(prefix) {
            return None;
        }
        let pad = if pad > 0 {
            pad.min(u8::MAX as usize) as u8
        } else {
            ItemKind::ALL
                .iter()
                .find(|k| k.prefix() == prefix)
                .map_or(3, |k| k.pad)
        };
        let mut p = Prefix {
            buf: [0; MAX_PREFIX],
            len: prefix.len() as u8,
        };
        p.buf[..prefix.len()].copy_from_slice(prefix.as_bytes());
        Some(ItemKind { prefix: p, pad })
    }

    /// Builtin index in [`ItemKind::ALL`] order — `None` for
    /// user/undeclared prefixes.
    pub fn builtin_index(&self) -> Option<usize> {
        ItemKind::ALL.iter().position(|k| *k == *self)
    }

    /// Serde/config name — `task`, `issue`, … for builtins; the
    /// bare prefix otherwise (the real name lives in the table —
    /// [`crate::KindTable::name_of`]).
    pub fn name(&self) -> &str {
        match self.builtin_index() {
            Some(0) => "task",
            Some(1) => "issue",
            Some(2) => "proposal",
            Some(3) => "adr",
            Some(4) => "plan",
            _ => self.prefix(),
        }
    }
}

impl PartialEq for ItemKind {
    /// Prefix identity — `pad` is a display hint, not identity.
    fn eq(&self, other: &Self) -> bool {
        self.prefix() == other.prefix()
    }
}

impl Eq for ItemKind {}

impl std::hash::Hash for ItemKind {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        self.prefix().hash(h);
    }
}

impl PartialOrd for ItemKind {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ItemKind {
    /// Builtin-declaration order (`Task < Issue < … < Plan`),
    /// then prefix-alphabetical for user kinds — context-free so
    /// `BTreeSet` order can't depend on a kind table.
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        let rank = |k: &ItemKind| k.builtin_index().unwrap_or(ItemKind::ALL.len());
        rank(self)
            .cmp(&rank(other))
            .then_with(|| self.prefix().cmp(other.prefix()))
    }
}

impl fmt::Debug for ItemKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("ItemKind").field(&self.prefix()).finish()
    }
}

/// Failure to parse an [`ItemKind`] name or prefix.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid item kind {0:?} — expected task|issue|proposal|adr|plan or a declared prefix")]
pub struct ParseItemKindError(pub String);

impl FromStr for ItemKind {
    type Err = ParseItemKindError;

    /// Builtin serde names (`task`, …) or any valid prefix
    /// (`T`, `S`) — user-kind *names* resolve via the table.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        ItemKind::ALL
            .into_iter()
            .find(|k| k.name() == s)
            .or_else(|| ItemKind::from_prefix(s))
            .ok_or_else(|| ParseItemKindError(s.to_string()))
    }
}

impl serde::Serialize for ItemKind {
    /// Builtin name, or the prefix for user kinds.
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self.name())
    }
}

impl<'de> serde::Deserialize<'de> for ItemKind {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(feature = "schema")]
impl schemars::JsonSchema for ItemKind {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "ItemKind".into()
    }

    fn json_schema(gen: &mut schemars::SchemaGenerator) -> schemars::Schema {
        String::json_schema(gen)
    }
}

/// A typed, numbered id: kind + counter. `Ord` sorts by kind, then
/// number — `T-002 < T-010`, and `T-010 < I-001` across kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ItemId {
    kind: ItemKind,
    number: u32,
}

impl ItemId {
    /// Construct without validation (`number == 0` is allowed for
    /// internal use; `Display` still pads it).
    pub const fn new(kind: ItemKind, number: u32) -> Self {
        ItemId { kind, number }
    }

    /// The kind segment.
    pub fn kind(self) -> ItemKind {
        self.kind
    }

    /// The counter segment.
    pub fn number(self) -> u32 {
        self.number
    }

    /// The id that follows this one within the same kind.
    pub fn next(self) -> ItemId {
        ItemId {
            kind: self.kind,
            number: self.number.saturating_add(1),
        }
    }
}

/// Verbs are kind-locked — `myco issue resolve T-001` is a usage
/// error, not a lookup failure.
pub fn require_kind(id: ItemId, kind: ItemKind, verb: &str) -> Result<(), String> {
    if id.kind() == kind {
        Ok(())
    } else {
        Err(format!(
            "{verb} takes {}-NNN ids — {id} is a {} item",
            kind.prefix(),
            id.kind().prefix()
        ))
    }
}

/// Failure to parse an [`ItemId`] — bad prefix, empty/bad number,
/// or `0`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid item id {0:?} — expected <prefix>-<number> like T-012 or ADR-0004")]
pub struct ParseItemIdError(pub String);

impl FromStr for ItemId {
    type Err = ParseItemIdError;

    /// `<prefix>-<number>` — the grammar is lenient about which
    /// prefixes exist: any `[A-Z][A-Z0-9]*` parses; whether the
    /// prefix is *declared* is checked where a kind table is in
    /// scope (`check`, the store), not here — a foreign-prefix id
    /// in `blocks:` must parse so the ledger stays readable.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let bad = || ParseItemIdError(s.to_string());
        let (prefix, digits) = s.split_once('-').ok_or_else(bad)?;
        let base = ItemKind::from_prefix(prefix).ok_or_else(bad)?;
        if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
            return Err(bad());
        }
        // width hint: builtins keep their declared pad; user kinds
        // take the digit-run floor so canonical ids round-trip
        let kind = if base.builtin_index().is_some() {
            base
        } else {
            ItemKind::for_spec(prefix, digits.len().max(3)).unwrap_or(base)
        };
        let number: u32 = digits.parse().map_err(|_| bad())?;
        if number == 0 {
            return Err(bad());
        }
        Ok(ItemId { kind, number })
    }
}

impl fmt::Display for ItemId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}-{:0width$}",
            self.kind.prefix(),
            self.number,
            width = self.kind.width()
        )
    }
}

impl serde::Serialize for ItemId {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> serde::Deserialize<'de> for ItemId {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(feature = "schema")]
impl schemars::JsonSchema for ItemId {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "ItemId".into()
    }

    fn json_schema(gen: &mut schemars::SchemaGenerator) -> schemars::Schema {
        String::json_schema(gen)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn parse_canonical() {
        assert_eq!(
            "T-012".parse::<ItemId>().unwrap(),
            ItemId::new(ItemKind::Task, 12)
        );
        assert_eq!(
            "I-003".parse::<ItemId>().unwrap(),
            ItemId::new(ItemKind::Issue, 3)
        );
        assert_eq!(
            "P-002".parse::<ItemId>().unwrap(),
            ItemId::new(ItemKind::Proposal, 2)
        );
        assert_eq!(
            "ADR-0004".parse::<ItemId>().unwrap(),
            ItemId::new(ItemKind::Adr, 4)
        );
        assert_eq!(
            "PL-001".parse::<ItemId>().unwrap(),
            ItemId::new(ItemKind::Plan, 1)
        );
    }

    #[test]
    fn parse_accepts_noncanonical_width() {
        // Longer/shorter digit runs parse fine; emit re-canonicalizes.
        assert_eq!(
            "T-7".parse::<ItemId>().unwrap(),
            ItemId::new(ItemKind::Task, 7)
        );
        assert_eq!(
            "ADR-12".parse::<ItemId>().unwrap(),
            ItemId::new(ItemKind::Adr, 12)
        );
        assert_eq!(
            "T-01234".parse::<ItemId>().unwrap(),
            ItemId::new(ItemKind::Task, 1234)
        );
    }

    #[test]
    fn parse_accepts_user_prefixes() {
        // the grammar is lenient — declared-ness is the table's
        // job; `S-7` gets a floor-3 pad hint, `S-0007` keeps 4
        let s = "S-7".parse::<ItemId>().unwrap();
        assert_eq!(s.number(), 7);
        assert_eq!(s.kind().prefix(), "S");
        assert_eq!(s.to_string(), "S-007");
        assert_eq!("S-0007".parse::<ItemId>().unwrap().to_string(), "S-0007");
        // same id regardless of pad hint
        assert_eq!(s, "S-007".parse::<ItemId>().unwrap());
        assert!(ItemKind::from_prefix("BATCH").is_some());
        assert!(ItemKind::from_prefix("BATCH-X").is_none()); // charset is [A-Z0-9]
    }

    #[test]
    fn parse_rejects() {
        for bad in [
            "",
            "T",
            "T-",
            "-001",
            "t-001",
            "T-abc",
            "T-0",
            "T-00",
            "T-1-2",
            "T-99999999999999999999",
            "ADR-0000",
            "T–001",
            "A-1-",
        ] {
            assert!(bad.parse::<ItemId>().is_err(), "{bad} should fail");
        }
        // X-001 parses now — undeclared is a table-level finding
        assert!("X-001".parse::<ItemId>().is_ok());
        // but malformed prefixes still fail
        assert!(ItemKind::from_prefix("X ").is_none());
        assert!(ItemKind::from_prefix("").is_none());
        assert!(ItemKind::from_prefix("TOOLONGPREFIXXXXXX").is_none());
    }

    #[test]
    fn display_pads() {
        assert_eq!(ItemId::new(ItemKind::Task, 12).to_string(), "T-012");
        assert_eq!(ItemId::new(ItemKind::Adr, 4).to_string(), "ADR-0004");
        assert_eq!(ItemId::new(ItemKind::Task, 1234).to_string(), "T-1234");
        assert_eq!(ItemId::new(ItemKind::Adr, 12345).to_string(), "ADR-12345");
        // spec-declared pad honored
        let s = ItemKind::for_spec("S", 4).unwrap();
        assert_eq!(ItemId::new(s, 7).to_string(), "S-0007");
    }

    #[test]
    fn ordering() {
        assert!(ItemId::new(ItemKind::Task, 2) < ItemId::new(ItemKind::Task, 10));
        // kinds sort in declaration order: Task < Issue < Proposal < Adr < Plan
        assert!(ItemId::new(ItemKind::Task, 999) < ItemId::new(ItemKind::Issue, 1));
        assert!(ItemId::new(ItemKind::Adr, 1) < ItemId::new(ItemKind::Plan, 1));
        // user kinds trail the prelude, alphabetical by prefix
        let s = ItemKind::from_prefix("S").unwrap();
        let z = ItemKind::from_prefix("Z").unwrap();
        assert!(ItemId::new(ItemKind::Plan, 99) < ItemId::new(s, 1));
        assert!(ItemId::new(s, 9) < ItemId::new(z, 1));
    }

    #[test]
    fn require_kind_message() {
        let t = ItemId::new(ItemKind::Task, 1);
        assert!(require_kind(t, ItemKind::Task, "todo").is_ok());
        let i = ItemId::new(ItemKind::Issue, 1);
        let e = require_kind(i, ItemKind::Task, "todo").unwrap_err();
        assert!(e.contains("T-NNN"), "{e}");
    }

    #[test]
    fn serde_string() {
        let id = ItemId::new(ItemKind::Adr, 4);
        assert_eq!(serde_json::to_string(&id).unwrap(), "\"ADR-0004\"");
        assert_eq!(serde_json::from_str::<ItemId>("\"ADR-0004\"").unwrap(), id);
        assert!(serde_json::from_str::<ItemId>("\"nope\"").is_err());
        // user-kind ids round-trip through serde
        let s = "S-007".parse::<ItemId>().unwrap();
        assert_eq!(serde_json::from_str::<ItemId>("\"S-007\"").unwrap(), s);
    }

    #[test]
    fn kind_helpers() {
        assert_eq!(ItemKind::Task.prefix(), "T");
        assert_eq!(ItemKind::Adr.width(), 4);
        assert_eq!(ItemKind::Issue.width(), 3);
        assert_eq!(ItemKind::Task.counter_key(), "t");
        assert_eq!(ItemKind::Issue.counter_key(), "i");
        assert_eq!(ItemKind::Proposal.counter_key(), "p");
        assert_eq!(ItemKind::Adr.counter_key(), "adr");
        assert_eq!(ItemKind::Plan.counter_key(), "pl");
        assert_eq!(ItemKind::from_prefix("PL").unwrap(), ItemKind::Plan);
        assert_eq!(ItemKind::from_prefix("X").unwrap().prefix(), "X");
        assert!(ItemKind::from_prefix("").is_none());
        assert_eq!("task".parse::<ItemKind>().unwrap(), ItemKind::Task);
        assert_eq!("I".parse::<ItemKind>().unwrap(), ItemKind::Issue);
        assert!("bogus".parse::<ItemKind>().is_err());
        assert_eq!(
            ItemId::new(ItemKind::Task, 9).next(),
            ItemId::new(ItemKind::Task, 10)
        );
        // pad is a hint, not identity
        let a = ItemKind::for_spec("S", 3).unwrap();
        let b = ItemKind::for_spec("S", 4).unwrap();
        assert_eq!(a, b);
        assert_eq!(ItemId::new(a, 1), ItemId::new(b, 1));
    }

    #[cfg(feature = "schema")]
    #[test]
    fn json_schema_compiles() {
        let schema = schemars::schema_for!(ItemId);
        assert!(serde_json::to_value(&schema).is_ok());
        let kind_schema = schemars::schema_for!(ItemKind);
        assert!(serde_json::to_value(&kind_schema).is_ok());
    }
}
