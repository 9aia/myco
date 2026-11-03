//! refs — the pure edge scanner behind the link index (P-005).
//!
//! Every `project/` file contributes a set of [`Edge`]s, always
//! with `src` = the item the file speaks for: `blocks:`/
//! `superseded_by:` frontmatter declares edges verbatim, `X-NNN`
//! tokens in prose become `references` edges, and a
//! `handoffs/<ID>.md` filename is a `handoff` edge (a filename
//! fact — callers record it, nothing to scan).
//!
//! The index stores these forward edges plus a reverse table, so
//! `blocked_by`/`referenced_by`/`superseded_by` appear only as
//! mirrored kinds in query results — a file never declares an
//! incoming edge. Frontmatter edges are kept as declared
//! (self-blocks are the ledger's problem); mention self-hits are
//! dropped — quoting your own id isn't a link.
//!
//! Bounds (the proposal's "bounded GC"): bodies scan at most
//! [`MAX_SCAN_BYTES`] and emit at most [`MAX_MENTIONS`] reference
//! edges, so one pathological file can't blow up the index.

use std::collections::BTreeSet;

use crate::{Item, ItemId, KindTable};

/// The most body text one extract scans — 64 KiB. The cap applies
/// to the byte window, not to items: a file is still scanned
/// per-item, so every item gets its own 64 KiB.
pub const MAX_SCAN_BYTES: usize = 64 * 1024;

/// The most `references` edges one extract emits.
pub const MAX_MENTIONS: usize = 64;

/// The relationship an [`Edge`] asserts between two items. The
/// `*_by` forms are the reverse views — extract only ever produces
/// [`Handoff`](EdgeKind::Handoff), [`Blocks`](EdgeKind::Blocks),
/// [`Supersedes`](EdgeKind::Supersedes),
/// [`SupersededBy`](EdgeKind::SupersededBy), and
/// [`References`](EdgeKind::References); the index materializes
/// the mirror at query time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    /// `handoffs/<ID>.md` exists — `src == dst`, an attachment
    /// marker rather than a link between items.
    Handoff,
    /// `src` declares `blocks: dst`.
    Blocks,
    /// Reverse of [`Blocks`](EdgeKind::Blocks) — query-side only.
    BlockedBy,
    /// `src` declares `supersedes: dst`.
    Supersedes,
    /// `src` declares `superseded_by: dst`.
    SupersededBy,
    /// `src`'s text mentions `dst`.
    References,
    /// Reverse of [`References`](EdgeKind::References) —
    /// query-side only.
    ReferencedBy,
}

impl EdgeKind {
    /// All kinds, in declaration order.
    pub const ALL: [EdgeKind; 7] = [
        EdgeKind::Handoff,
        EdgeKind::Blocks,
        EdgeKind::BlockedBy,
        EdgeKind::Supersedes,
        EdgeKind::SupersededBy,
        EdgeKind::References,
        EdgeKind::ReferencedBy,
    ];

    /// Stable token — `handoff`, `blocks`, `blocked_by`, … Used as
    /// the storage key segment and the display label.
    pub fn name(self) -> &'static str {
        match self {
            EdgeKind::Handoff => "handoff",
            EdgeKind::Blocks => "blocks",
            EdgeKind::BlockedBy => "blocked_by",
            EdgeKind::Supersedes => "supersedes",
            EdgeKind::SupersededBy => "superseded_by",
            EdgeKind::References => "references",
            EdgeKind::ReferencedBy => "referenced_by",
        }
    }

    /// Kind from a [`name`](EdgeKind::name) token; `None` when
    /// unknown (forward-compat: a newer edge kind in an old
    /// index reads as "skipped", not corrupt).
    pub fn from_name(name: &str) -> Option<EdgeKind> {
        EdgeKind::ALL.into_iter().find(|k| k.name() == name)
    }

    /// The reverse view: `Blocks` ↔ `BlockedBy`,
    /// `Supersedes` ↔ `SupersededBy`,
    /// `References` ↔ `ReferencedBy`; `Handoff` mirrors to itself.
    /// An incoming `(other → id, k)` edge reads as
    /// `(id, other, k.mirror())` to the item being queried.
    pub fn mirror(self) -> EdgeKind {
        match self {
            EdgeKind::Blocks => EdgeKind::BlockedBy,
            EdgeKind::BlockedBy => EdgeKind::Blocks,
            EdgeKind::Supersedes => EdgeKind::SupersededBy,
            EdgeKind::SupersededBy => EdgeKind::Supersedes,
            EdgeKind::References => EdgeKind::ReferencedBy,
            EdgeKind::ReferencedBy => EdgeKind::References,
            EdgeKind::Handoff => EdgeKind::Handoff,
        }
    }

    /// Can `extract` produce this kind? The `*_by` forms have no
    /// frontmatter to declare them (`blocked_by` exists only as a
    /// `blocks` mirror) — except [`SupersededBy`](EdgeKind::SupersededBy),
    /// which *is* a real frontmatter field on ADR-shaped items.
    pub fn extracted(self) -> bool {
        !matches!(self, EdgeKind::BlockedBy | EdgeKind::ReferencedBy)
    }
}

/// One directed edge `src → dst` — the unit `extract` emits and
/// the index stores. `Ord` sorts `(src, kind, dst)` for
/// deterministic output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Edge {
    /// The item the file speaks for.
    pub src: ItemId,
    /// The item `src` points at.
    pub dst: ItemId,
    /// What `src` asserts about `dst`.
    pub kind: EdgeKind,
}

impl PartialOrd for Edge {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Edge {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (self.src, self.kind, self.dst).cmp(&(other.src, other.kind, other.dst))
    }
}

impl Edge {
    /// `src --kind--> dst`.
    pub const fn new(src: ItemId, dst: ItemId, kind: EdgeKind) -> Self {
        Edge { src, dst, kind }
    }

    /// The `handoffs/<ID>.md` association — a self-edge recording
    /// that `id` has handoff notes on disk. A filename fact: the
    /// caller knows it, nothing to parse.
    pub const fn handoff(id: ItemId) -> Self {
        Edge::new(id, id, EdgeKind::Handoff)
    }
}

/// The edges one file (or file section) declares for `id` —
/// frontmatter links plus `X-NNN` mentions in `body`. `meta` is
/// `None` for files without item frontmatter (handoffs); `id` is
/// always the item the file speaks for (the caller resolves it —
/// from frontmatter, filename, or ledger position).
///
/// Pure and deterministic: same input, same edge set. Dedup'd —
/// mentioning `T-001` ten times is one edge. Mentioning `id`
/// itself yields nothing (a self-mention isn't a link).
///
/// `kinds` gates the *prose* scan: since P-008 any `[A-Z]`-prefixed
/// token is a grammatical id, so a mention only counts when its
/// prefix is a declared kind — `WIP-1` in prose stays prose.
/// Frontmatter edges are declared data and never filtered.
pub fn extract(body: &str, meta: Option<&Item>, id: ItemId, kinds: &KindTable) -> BTreeSet<Edge> {
    let mut edges = BTreeSet::new();
    if let Some(m) = meta {
        for b in &m.blocks {
            edges.insert(Edge::new(id, *b, EdgeKind::Blocks));
        }
        if let Some(s) = m.supersedes {
            edges.insert(Edge::new(id, s, EdgeKind::Supersedes));
        }
        if let Some(s) = m.superseded_by {
            edges.insert(Edge::new(id, s, EdgeKind::SupersededBy));
        }
    }
    for dst in mentions(body) {
        if dst != id && kinds.spec_of(dst.kind()).is_some() {
            edges.insert(Edge::new(id, dst, EdgeKind::References));
        }
    }
    edges
}

/// `X-NNN` id tokens in `text` — `T-001`, `I-003`, `P-002`,
/// `ADR-0004`, `PL-001`, plus any ≤3-char uppercase prefix a
/// user kind might declare (P-008 — the scan is grammar-level;
/// [`extract`] drops undeclared prefixes via the kind table).
/// Deliberately not markdown-aware: a token in a code fence
/// counts the same as one in prose (v1 deliberately over-counts
/// rather than teaching the scanner the grammar). Sorted,
/// dedup'd, bounded by [`MAX_SCAN_BYTES`] / [`MAX_MENTIONS`];
/// malformed candidates (`T-0`, overflow numbers) fall through
/// [`ItemId`]'s own parser.
pub fn mentions(text: &str) -> Vec<ItemId> {
    let b = &text.as_bytes()[..text.len().min(MAX_SCAN_BYTES)];
    let mut out = BTreeSet::new();
    let mut i = 0;
    while i < b.len() && out.len() < MAX_MENTIONS {
        if !b[i].is_ascii_uppercase() {
            i += 1;
            continue;
        }
        // an uppercase run is the prefix candidate — it must start
        // on a token boundary so `XT-001` doesn't yield `T-001`
        // via the `T` inside it
        let start = i;
        while i < b.len() && b[i].is_ascii_uppercase() {
            i += 1;
        }
        let bounded_before = start == 0 || !b[start - 1].is_ascii_alphanumeric();
        if !bounded_before || !(1..=3).contains(&(i - start)) || b.get(i) != Some(&b'-') {
            continue;
        }
        let dstart = i + 1;
        let mut d = dstart;
        while d < b.len() && b[d].is_ascii_digit() {
            d += 1;
        }
        // the token must end cleanly — `T-001x` isn't `T-001`,
        // and a trailing `-` means a longer token we don't own
        let bounded_after = d == b.len() || !(b[d].is_ascii_alphanumeric() || b[d] == b'-');
        if d > dstart && bounded_after {
            // [start..d] is ASCII — a valid str slice boundary
            if let Ok(id) = text[start..d].parse::<ItemId>() {
                out.insert(id);
            }
        }
        i = d; // consumed the digits either way — no re-scan inside
    }
    out.into_iter().collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::{ItemKind, KindTable};

    fn kinds() -> KindTable {
        KindTable::prelude()
    }

    fn t(n: u32) -> ItemId {
        ItemId::new(ItemKind::Task, n)
    }

    fn i(n: u32) -> ItemId {
        ItemId::new(ItemKind::Issue, n)
    }

    #[test]
    fn kind_names_and_mirror() {
        for k in EdgeKind::ALL {
            assert_eq!(EdgeKind::from_name(k.name()), Some(k), "{}", k.name());
            assert_eq!(k.mirror().mirror(), k);
        }
        assert_eq!(EdgeKind::from_name("relates"), None);
        assert!(EdgeKind::BlockedBy.mirror() == EdgeKind::Blocks);
        assert!(!EdgeKind::BlockedBy.extracted());
        assert!(EdgeKind::SupersededBy.extracted());
    }

    #[test]
    fn extract_frontmatter_edges() {
        let meta = Item::new(t(1), "x")
            .blocks(t(2))
            .blocks(i(3))
            .supersedes(ItemId::new(ItemKind::Adr, 1))
            .superseded_by(ItemId::new(ItemKind::Adr, 9));
        let edges = extract("", Some(&meta), t(1), &kinds());
        assert_eq!(
            edges,
            BTreeSet::from([
                Edge::new(t(1), t(2), EdgeKind::Blocks),
                Edge::new(t(1), i(3), EdgeKind::Blocks),
                Edge::new(t(1), ItemId::new(ItemKind::Adr, 1), EdgeKind::Supersedes),
                Edge::new(t(1), ItemId::new(ItemKind::Adr, 9), EdgeKind::SupersededBy),
            ])
        );
    }

    #[test]
    fn extract_mentions_dedups_and_drops_self() {
        // no meta — the handoff-file shape
        let edges = extract(
            "see T-001, T-001 again, also I-003 and I-003",
            None,
            t(2),
            &kinds(),
        );
        assert_eq!(
            edges,
            BTreeSet::from([
                Edge::new(t(2), t(1), EdgeKind::References),
                Edge::new(t(2), i(3), EdgeKind::References),
            ])
        );
        // own id in prose is not an edge
        let edges = extract("T-002 (that's me) relates to T-001", None, t(2), &kinds());
        assert_eq!(
            edges,
            BTreeSet::from([Edge::new(t(2), t(1), EdgeKind::References)])
        );
    }

    #[test]
    fn mentions_all_kinds_and_boundaries() {
        let ids = mentions("T-001 I-3 P-002 ADR-0004 PL-001");
        assert_eq!(
            ids,
            [
                t(1),
                i(3),
                ItemId::new(ItemKind::Proposal, 2),
                ItemId::new(ItemKind::Adr, 4),
                ItemId::new(ItemKind::Plan, 1)
            ]
        );
        // code-fence tokens count the same as prose (v1 rule)
        assert_eq!(mentions("```\nT-007\n```"), [t(7)]);
        // the scanner's own bound: prefix runs past 3 chars are
        // never candidates — `FOOADR-0001` can't yield `ADR-0001`
        assert!(mentions("FOOADR-0001").is_empty());
        // short foreign prefixes parse (grammar-level leniency,
        // P-008) — `extract` drops them via the kind table
        let foreign = mentions("XT-001 NOT-002");
        assert_eq!(foreign.len(), 2);
        assert!(extract("XT-001 NOT-002", None, t(2), &kinds()).is_empty());
        // a declared user kind's mentions DO become edges
        let spike = toml::from_str::<crate::KindSpec>(
            "name = \"xt\"\nprefix = \"XT\"\nstorage = \"doc\"\n",
        )
        .unwrap();
        let kinds2 = KindTable::new(vec![spike]).unwrap();
        let edges = extract("see XT-001", None, t(2), &kinds2);
        assert_eq!(
            edges,
            BTreeSet::from([Edge::new(
                t(2),
                "XT-001".parse().unwrap(),
                EdgeKind::References
            )])
        );
        // longer tokens we don't own — `T-001x`, `T-001-2`
        assert!(mentions("T-001x T-001-2").is_empty());
        // but a real mention after a separator still lands
        assert_eq!(mentions("WELL-T-005,"), [t(5)]);
        // zero / overflow / malformed rejected by ItemId's parser —
        // `X-001` itself parses now (grammar leniency)
        assert!(
            mentions("T-0 T-00 X-001 T-99999999999999999999").contains(&"X-001".parse().unwrap())
        );
        assert!(mentions("T-0 T-00 T-99999999999999999999").is_empty());
        // punctuation boundaries are fine
        assert_eq!(mentions("(T-004), [I-009]."), [t(4), i(9)]);
    }

    #[test]
    fn mentions_bounds() {
        // over the mention cap the tail is just not recorded
        let many: String = (1..=MAX_MENTIONS + 10).map(|n| format!("I-{n} ")).collect();
        assert_eq!(mentions(&many).len(), MAX_MENTIONS);
        // past the byte cap nothing is scanned
        let pad = "x".repeat(MAX_SCAN_BYTES);
        assert!(mentions(&format!("{pad}T-001")).is_empty());
        // right at the cap boundary the token still lands
        let pad = format!("{} ", "x".repeat(MAX_SCAN_BYTES - "T-001".len() - 1));
        assert_eq!(mentions(&format!("{pad}T-001")), [t(1)]);
    }

    #[test]
    fn handoff_is_a_self_edge() {
        let e = Edge::handoff(i(2));
        assert_eq!(e.src, i(2));
        assert_eq!(e.dst, i(2));
        assert_eq!(e.kind, EdgeKind::Handoff);
    }
}
