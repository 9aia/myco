//! Declarative views — `filter → group → sort → limit → format`
//! as data (P-006). A [`ViewSpec`] is pure config; [`eval`] is a
//! pure fn over `&[Item]`. Named views come from `[views]` in
//! myco.toml layers; the built-in presets (`board`, `epic`, …)
//! are the same engine's shapes as data.
//!
//! Semantics: OR within a filter list, AND across fields. Filters
//! see top-level items only — children render with their parent.

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::{Actor, Item, ItemKind, KindTable, Priority, Status};

/// Where a view's item universe comes from.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum ViewSource {
    /// This repo's `project/` worktree (the default).
    #[default]
    Repo,
    /// The union of every index-registered repo's snapshot —
    /// the cross-repo "org" universe; items carry a repo tag
    /// driving `group_by = "repo"` and `repo:ID` card prefixes.
    Index,
}

/// Failure to parse a [`ViewSource`] name.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid source {0:?} — expected repo|index")]
pub struct ParseViewSourceError(pub String);

impl ViewSource {
    /// The config/flag name.
    pub fn name(self) -> &'static str {
        match self {
            ViewSource::Repo => "repo",
            ViewSource::Index => "index",
        }
    }

    /// Default-source check — `serde(skip_serializing_if)` hook
    /// keeping `repo` out of emitted specs.
    pub fn is_default(&self) -> bool {
        *self == ViewSource::Repo
    }
}

impl FromStr for ViewSource {
    type Err = ParseViewSourceError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "repo" => Ok(ViewSource::Repo),
            "index" => Ok(ViewSource::Index),
            _ => Err(ParseViewSourceError(s.to_string())),
        }
    }
}

/// A named view: what to show and how to render it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ViewSpec {
    /// Item selection — absent fields match everything.
    #[serde(default)]
    pub filter: ViewFilter,
    /// Universe — `repo` (default) or `index` (every registered
    /// repo's snapshot unioned; items are repo-tagged).
    #[serde(default, skip_serializing_if = "ViewSource::is_default")]
    pub source: ViewSource,
    /// Grouping axis; `board` defaults this to `status`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_by: Option<GroupAxis>,
    /// Ordered sort keys — empty means the default `pri,id`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sort: Vec<SortKey>,
    /// Post-sort truncation across the flattened result.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    /// Renderer name.
    #[serde(default)]
    pub format: ViewFormat,
}

/// Item selection — the `filter = { … }` inline table.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ViewFilter {
    /// `kind` — kind names or id prefixes (`task`, `issue`,
    /// `spike`, `S`). Kept as tokens, not a closed kind set —
    /// user kinds (P-008) resolve through the [`KindTable`] at
    /// eval time.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub kind: Vec<String>,
    /// `status` — status tokens.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub status: Vec<Status>,
    /// `pri_min` — inclusive lower severity bound (crit > high).
    #[serde(
        default,
        deserialize_with = "de_opt_priority",
        skip_serializing_if = "Option::is_none"
    )]
    pub pri_min: Option<Priority>,
    /// `pri_max` — inclusive upper severity bound.
    #[serde(
        default,
        deserialize_with = "de_opt_priority",
        skip_serializing_if = "Option::is_none"
    )]
    pub pri_max: Option<Priority>,
    /// `label` — item carries any of these `#label`s.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub label: Vec<String>,
    /// `owner` — item's owner is any of these actors.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub owner: Vec<Actor>,
    /// `claimed` — `false` selects ownerless items. Mutually
    /// exclusive with `owner` (validated).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claimed: Option<bool>,
    /// `epic` — exact epic name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub epic: Option<String>,
    /// `milestone` — exact milestone name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub milestone: Option<String>,
    /// `due` — exact due date (`"YYYY-MM-DD"`).
    #[serde(
        default,
        with = "crate::date::opt",
        skip_serializing_if = "Option::is_none"
    )]
    #[cfg_attr(feature = "schema", schemars(with = "Option<String>"))]
    pub due: Option<time::Date>,
    /// `estimate` — exact point estimate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estimate: Option<u32>,
    /// `cycle` — exact cycle name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cycle: Option<String>,
    /// `superseded` — `false` excludes items with `superseded_by`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub superseded: Option<bool>,
}

/// One grouping axis for `group_by`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum GroupAxis {
    /// Lifecycle columns (the board's default).
    Status,
    /// Severity buckets — rendered crit-first.
    Priority,
    /// Item-kind buckets in declaration order (task → plan).
    Kind,
    /// Owner buckets, alphabetical.
    Owner,
    /// Epic buckets, alphabetical.
    Epic,
    /// Milestone buckets, alphabetical.
    Milestone,
    /// Source-repo buckets, alphabetical — the index-universe
    /// axis; untagged (repo-sourced) items land in `ungrouped`.
    Repo,
    /// Cycle buckets (`cycle:<name>`), alphabetical — declared
    /// windows live in `myco.toml [[cycles]]`; undeclared names
    /// still group (free-form names are legal).
    Cycle,
}

/// The renderer a view selects.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum ViewFormat {
    /// Flat lines; headed groups when `group_by` is set.
    #[default]
    List,
    /// Columnar — `group_by` supplies the column axis.
    Board,
    /// One row per repo, not item groups — an `index` source emits
    /// a row per registered repo (snapshot counts + error), a
    /// `repo` source a single row for this repo.
    Repos,
    /// Roadmap/timeline emit (P-012) — epic → milestone → cycle
    /// lanes with items placed by `due:`; not a group render.
    Timeline,
    /// Count-by-dimension emit — status/priority/owner/epic/
    /// milestone/cycle/label tallies; ASCII bars on tty, JSON on
    /// `--json`.
    Stats,
    /// Cumulative done-over-time vs total scope, per cycle when
    /// `[[cycles]]` declares them — driven by `done:` stamps;
    /// unstamped archive predates the field and counts toward
    /// scope only.
    Burnup,
}

/// Sortable fields for [`SortKey`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortField {
    /// Priority — default crit-first (matches `pri_then_id`).
    Pri,
    /// Typed id — kind then number, ascending.
    Id,
    /// `created:` date — ascending; absent dates sort last either way.
    Created,
    /// `due:` date — ascending; undated items sort last either way.
    Due,
    /// `est:` points — ascending; unestimated items sort last either way.
    Estimate,
    /// `cycle:` name — ascending; uncycled items sort last either way.
    Cycle,
    /// Lifecycle order (open → deferred).
    Status,
    /// ItemKind order (task → plan).
    Kind,
    /// Owner string, ascending; unclaimed items sort last.
    Owner,
    /// Title string, ascending.
    Title,
}

/// A sort key — `"pri"` or `"-created"` (`-` flips the field's
/// default direction).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SortKey {
    /// The field compared.
    pub field: SortField,
    /// true = reversed from the field's default direction.
    pub desc: bool,
}

/// Failure to parse a [`SortKey`] (`"wat"`, `"-"`).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid sort key {0:?} — expected pri|id|created|due|est|cycle|status|kind|owner|title, optionally -prefixed")]
pub struct ParseSortKeyError(pub String);

impl FromStr for SortKey {
    type Err = ParseSortKeyError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (desc, name) = match s.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, s),
        };
        let field = match name {
            "pri" => SortField::Pri,
            "id" => SortField::Id,
            "created" => SortField::Created,
            "due" => SortField::Due,
            "est" | "estimate" => SortField::Estimate,
            "cycle" => SortField::Cycle,
            "status" => SortField::Status,
            "kind" => SortField::Kind,
            "owner" => SortField::Owner,
            "title" => SortField::Title,
            _ => return Err(ParseSortKeyError(s.to_string())),
        };
        Ok(SortKey { field, desc })
    }
}

impl fmt::Display for SortKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.desc {
            write!(f, "-")?;
        }
        f.write_str(match self.field {
            SortField::Pri => "pri",
            SortField::Id => "id",
            SortField::Created => "created",
            SortField::Due => "due",
            SortField::Estimate => "estimate",
            SortField::Cycle => "cycle",
            SortField::Status => "status",
            SortField::Kind => "kind",
            SortField::Owner => "owner",
            SortField::Title => "title",
        })
    }
}

impl Serialize for SortKey {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for SortKey {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(feature = "schema")]
impl schemars::JsonSchema for SortKey {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "SortKey".into()
    }

    fn json_schema(gen: &mut schemars::SchemaGenerator) -> schemars::Schema {
        String::json_schema(gen)
    }
}

#[cfg(feature = "schema")]
impl schemars::JsonSchema for SortField {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "SortField".into()
    }

    fn json_schema(gen: &mut schemars::SchemaGenerator) -> schemars::Schema {
        String::json_schema(gen)
    }
}

/// Accepts `pri:` tokens (`med`, `crit`) and full serde names
/// (`medium`, `critical`) — lenient read, strict write.
fn de_opt_priority<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Priority>, D::Error> {
    Option::<String>::deserialize(d)?
        .map(|s| s.parse().map_err(serde::de::Error::custom))
        .transpose()
}

/// One evaluated item — `repo` tags index-sourced universes
/// (`source = "index"`) with the name of the repo each item's
/// snapshot came from; `None` for a repo universe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Scoped<'a> {
    /// The item.
    pub item: &'a Item,
    /// Source-repo name (index-sourced universes only).
    pub repo: Option<&'a str>,
}

impl<'a> Scoped<'a> {
    /// Untagged — an item from the repo's own worktree.
    pub fn new(item: &'a Item) -> Self {
        Scoped { item, repo: None }
    }

    /// Tagged with the item's source repo — the index universe.
    pub fn tagged(repo: &'a str, item: &'a Item) -> Self {
        Scoped {
            item,
            repo: Some(repo),
        }
    }
}

impl std::ops::Deref for Scoped<'_> {
    type Target = Item;

    fn deref(&self) -> &Item {
        self.item
    }
}

impl serde::Serialize for Scoped<'_> {
    /// The item's own object plus `"repo"` when tagged — the
    /// shape `org` views always emitted per item.
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Flat<'a> {
            #[serde(flatten)]
            item: &'a Item,
            #[serde(skip_serializing_if = "Option::is_none")]
            repo: Option<&'a str>,
        }
        Flat {
            item: self.item,
            repo: self.repo,
        }
        .serialize(s)
    }
}

/// One bucket of an evaluated view — `label` is the axis value
/// (`"in-progress"`, `"high"`, `"ungrouped"`); `None` for an
/// ungrouped (`list`) result.
#[derive(Debug, PartialEq, Eq)]
pub struct ViewGroup<'a> {
    /// Group label; `None` when the spec has no group axis.
    pub label: Option<String>,
    /// Matched items in sort order.
    pub items: Vec<Scoped<'a>>,
}

/// The label `group_by = "ungrouped"` items land under — always
/// the trailing bucket.
pub const UNGROUPED: &str = "ungrouped";

impl GroupAxis {
    /// Bucket key for ordering + label for display. `None` puts
    /// the item in the [`UNGROUPED`] bucket (only reachable for
    /// optional-field axes). `kinds` resolves the `kind` axis —
    /// declared order and names live in the table.
    fn key_label(self, scoped: &Scoped, kinds: &KindTable) -> (GroupKey, String) {
        let item = scoped.item;
        match self {
            GroupAxis::Status => (
                GroupKey::Finite(item.status as usize),
                item.status.token().to_string(),
            ),
            GroupAxis::Priority => (
                // crit-first within the axis
                GroupKey::Finite(usize::MAX - item.priority as usize),
                item.priority.name().to_string(),
            ),
            GroupAxis::Kind => match kinds.position(item.id.kind()) {
                Some(pos) => (GroupKey::Finite(pos), kinds.name_of(item.id.kind())),
                // undeclared-prefix item — a named bucket after
                // the declared kinds
                None => (
                    GroupKey::Named(item.id.kind().prefix().to_string()),
                    item.id.kind().prefix().to_string(),
                ),
            },
            GroupAxis::Owner => match &item.owner {
                Some(o) => (
                    GroupKey::Named(o.as_str().to_string()),
                    o.as_str().to_string(),
                ),
                None => (GroupKey::Ungrouped, UNGROUPED.to_string()),
            },
            GroupAxis::Epic => named_or_ungrouped(item.epic.as_deref()),
            GroupAxis::Milestone => named_or_ungrouped(item.milestone.as_deref()),
            GroupAxis::Repo => named_or_ungrouped(scoped.repo),
            GroupAxis::Cycle => named_or_ungrouped(item.cycle.as_deref()),
        }
    }
}

fn named_or_ungrouped(v: Option<&str>) -> (GroupKey, String) {
    match v {
        Some(s) => (GroupKey::Named(s.to_string()), s.to_string()),
        None => (GroupKey::Ungrouped, UNGROUPED.to_string()),
    }
}

/// Failure to parse a [`GroupAxis`] name.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid group axis {0:?} — expected status|priority|kind|owner|epic|milestone|repo|cycle")]
pub struct ParseGroupAxisError(pub String);

impl GroupAxis {
    /// All axes, for help/error text.
    pub const ALL: [GroupAxis; 8] = [
        GroupAxis::Status,
        GroupAxis::Priority,
        GroupAxis::Kind,
        GroupAxis::Owner,
        GroupAxis::Epic,
        GroupAxis::Milestone,
        GroupAxis::Repo,
        GroupAxis::Cycle,
    ];

    /// The config/flag name.
    pub fn name(self) -> &'static str {
        match self {
            GroupAxis::Status => "status",
            GroupAxis::Priority => "priority",
            GroupAxis::Kind => "kind",
            GroupAxis::Owner => "owner",
            GroupAxis::Epic => "epic",
            GroupAxis::Milestone => "milestone",
            GroupAxis::Repo => "repo",
            GroupAxis::Cycle => "cycle",
        }
    }
}

impl FromStr for GroupAxis {
    type Err = ParseGroupAxisError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        GroupAxis::ALL
            .into_iter()
            .find(|a| a.name() == s)
            .ok_or_else(|| ParseGroupAxisError(s.to_string()))
    }
}

/// Failure to parse a [`ViewFormat`] name.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid format {0:?} — expected list|board|repos|timeline|stats|burnup")]
pub struct ParseViewFormatError(pub String);

impl ViewFormat {
    /// The config/flag name.
    pub fn name(self) -> &'static str {
        match self {
            ViewFormat::List => "list",
            ViewFormat::Board => "board",
            ViewFormat::Repos => "repos",
            ViewFormat::Timeline => "timeline",
            ViewFormat::Stats => "stats",
            ViewFormat::Burnup => "burnup",
        }
    }
}

impl FromStr for ViewFormat {
    type Err = ParseViewFormatError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "list" => Ok(ViewFormat::List),
            "board" => Ok(ViewFormat::Board),
            "repos" => Ok(ViewFormat::Repos),
            "timeline" => Ok(ViewFormat::Timeline),
            "stats" => Ok(ViewFormat::Stats),
            "burnup" => Ok(ViewFormat::Burnup),
            _ => Err(ParseViewFormatError(s.to_string())),
        }
    }
}

/// Bucket ordering: finite axes in canonical order, named axes
/// alphabetical, ungrouped always last.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum GroupKey {
    Finite(usize),
    Named(String),
    Ungrouped,
}

impl ViewFilter {
    /// Does `token` name this item's kind? Resolution order:
    /// declared name → declared prefix → a raw prefix compare, so
    /// `kind = ["X"]` still selects undeclared-prefix items.
    fn kind_matches(&self, token: &str, item: &Item, kinds: &KindTable) -> bool {
        match kinds.resolve(token) {
            Some(spec) => spec.id_kind() == item.id.kind(),
            None => token == item.id.kind().prefix(),
        }
    }

    /// All fields ANDed; each list ORs internally. An all-default
    /// filter matches everything. `kinds` resolves `kind` tokens
    /// — names and prefixes both accepted.
    pub fn matches(&self, item: &Item, kinds: &KindTable) -> bool {
        (self.kind.is_empty() || self.kind.iter().any(|t| self.kind_matches(t, item, kinds)))
            && (self.status.is_empty() || self.status.contains(&item.status))
            && self.pri_min.is_none_or(|p| item.priority >= p)
            && self.pri_max.is_none_or(|p| item.priority <= p)
            && (self.label.is_empty() || self.label.iter().any(|l| item.labels.contains(l)))
            && (self.owner.is_empty()
                || item.owner.as_ref().is_some_and(|o| self.owner.contains(o)))
            && self.claimed.is_none_or(|c| c == item.owner.is_some())
            && self
                .epic
                .as_ref()
                .is_none_or(|e| item.epic.as_ref() == Some(e))
            && self
                .milestone
                .as_ref()
                .is_none_or(|m| item.milestone.as_ref() == Some(m))
            && self.due.is_none_or(|d| item.due == Some(d))
            && self.estimate.is_none_or(|e| item.estimate == Some(e))
            && self
                .cycle
                .as_ref()
                .is_none_or(|c| item.cycle.as_ref() == Some(c))
            && self
                .superseded
                .is_none_or(|s| s == item.superseded_by.is_some())
    }
}

impl ViewSpec {
    /// Spec-level invariants broken combos can't reach eval:
    /// `claimed` + `owner` together, or `pri_min > pri_max`.
    pub fn validate(&self) -> Result<(), String> {
        if self.filter.claimed.is_some() && !self.filter.owner.is_empty() {
            return Err("claimed and owner are mutually exclusive".into());
        }
        if let (Some(lo), Some(hi)) = (self.filter.pri_min, self.filter.pri_max) {
            if lo > hi {
                return Err(format!(
                    "pri_min {} exceeds pri_max {}",
                    lo.name(),
                    hi.name()
                ));
            }
        }
        Ok(())
    }

    /// `kind` filter tokens must resolve to a declared kind or a
    /// grammatically valid prefix — `kind = ["tsak"]` is a typo
    /// to surface at resolution, not a silent empty view.
    pub fn validate_kinds(&self, kinds: &KindTable) -> Result<(), String> {
        for t in &self.filter.kind {
            if kinds.resolve(t).is_none() && ItemKind::from_prefix(t).is_none() {
                return Err(format!("unknown kind {t:?} in view filter"));
            }
        }
        Ok(())
    }

    /// `group_by`, or `status` when the format needs an axis
    /// (board is columnar — it must group by *something*).
    /// `repos` emits repo rows; `timeline`/`stats`/`burnup` are
    /// analytics emits — none of them group.
    pub fn effective_group_by(&self) -> Option<GroupAxis> {
        self.group_by.or(match self.format {
            ViewFormat::Board => Some(GroupAxis::Status),
            ViewFormat::List
            | ViewFormat::Repos
            | ViewFormat::Timeline
            | ViewFormat::Stats
            | ViewFormat::Burnup => None,
        })
    }

    /// The configured sort, or the default `pri,id` (crit-first,
    /// id ascending) when `sort` is empty — the order every other
    /// verb uses, so a view with no opinion still lands
    /// deterministically.
    pub fn effective_sort(&self) -> Vec<SortKey> {
        if self.sort.is_empty() {
            return [
                SortKey {
                    field: SortField::Pri,
                    desc: false,
                },
                SortKey {
                    field: SortField::Id,
                    desc: false,
                },
            ]
            .into();
        }
        self.sort.clone()
    }
}

/// `Option<T>` compare where `None` sorts last regardless of
/// direction — direction is applied to present values only.
fn opt_cmp<T: Ord>(a: Option<&T>, b: Option<&T>, desc: bool) -> Ordering {
    match (a, b) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some(a), Some(b)) => {
            if desc {
                b.cmp(a)
            } else {
                a.cmp(b)
            }
        }
    }
}

fn key_cmp(field: SortField, a: &Item, b: &Item, desc: bool) -> Ordering {
    let dir = |ord: Ordering| {
        if desc {
            ord.reverse()
        } else {
            ord
        }
    };
    match field {
        // pri default is crit-first — desc = low-first
        SortField::Pri => dir(b.priority.cmp(&a.priority)),
        SortField::Id => dir(a.id.cmp(&b.id)),
        SortField::Status => dir(a.status.cmp(&b.status)),
        SortField::Kind => dir(a.id.kind().cmp(&b.id.kind())),
        SortField::Title => dir(a.title.cmp(&b.title)),
        SortField::Created => opt_cmp(a.created.as_ref(), b.created.as_ref(), desc),
        SortField::Due => opt_cmp(a.due.as_ref(), b.due.as_ref(), desc),
        SortField::Estimate => opt_cmp(a.estimate.as_ref(), b.estimate.as_ref(), desc),
        SortField::Cycle => opt_cmp(a.cycle.as_ref(), b.cycle.as_ref(), desc),
        SortField::Owner => opt_cmp(a.owner.as_ref(), b.owner.as_ref(), desc),
    }
}

fn sort_items(items: &mut [Scoped], keys: &[SortKey]) {
    items.sort_by(|a, b| {
        keys.iter()
            .map(|k| key_cmp(k.field, a.item, b.item, k.desc))
            .find(|o| *o != Ordering::Equal)
            .unwrap_or(Ordering::Equal)
    });
}

/// Evaluate a spec over repo-sourced items — the
/// [`ViewSource::Repo`] universe. Every [`Scoped`] comes back
/// untagged (`repo: None`); [`eval_scoped`] is the tagged form.
/// `kinds` resolves the spec's `kind` filter and group axis.
pub fn eval<'a, I>(items: I, spec: &ViewSpec, kinds: &KindTable) -> Vec<ViewGroup<'a>>
where
    I: IntoIterator<Item = &'a Item>,
{
    eval_scoped(items.into_iter().map(Scoped::new), spec, kinds)
}

/// Evaluate a spec over [`Scoped`] items — the
/// [`ViewSource::Index`] universe feeds items tagged with their
/// source repo. Filter, bucket, sort each bucket, truncate.
/// Finite axes (status/priority/kind) yield every canonical
/// bucket even when empty; renderers decide whether to show
/// them. Named axes yield observed values only; [`UNGROUPED`]
/// trails when reached.
pub fn eval_scoped<'a, I>(items: I, spec: &ViewSpec, kinds: &KindTable) -> Vec<ViewGroup<'a>>
where
    I: IntoIterator<Item = Scoped<'a>>,
{
    let filtered: Vec<Scoped> = items
        .into_iter()
        .filter(|s| spec.filter.matches(s.item, kinds))
        .collect();
    let keys = spec.effective_sort();

    let mut groups: Vec<(GroupKey, String, Vec<Scoped>)> = match spec.effective_group_by() {
        None => filtered
            .is_empty()
            .then(Vec::new)
            .unwrap_or_else(|| vec![(GroupKey::Finite(0), String::new(), filtered)]),
        Some(axis) => {
            let mut buckets: BTreeMap<GroupKey, (String, Vec<Scoped>)> = match axis {
                GroupAxis::Status => Status::ALL
                    .iter()
                    .map(|s| {
                        (
                            GroupKey::Finite(*s as usize),
                            (s.token().to_string(), vec![]),
                        )
                    })
                    .collect(),
                GroupAxis::Priority => Priority::ALL
                    .iter()
                    .rev()
                    .map(|p| {
                        (
                            GroupKey::Finite(usize::MAX - *p as usize),
                            (p.name().to_string(), vec![]),
                        )
                    })
                    .collect(),
                GroupAxis::Kind => kinds
                    .iter()
                    .enumerate()
                    .map(|(i, spec)| (GroupKey::Finite(i), (spec.name.clone(), vec![])))
                    .collect(),
                GroupAxis::Owner
                | GroupAxis::Epic
                | GroupAxis::Milestone
                | GroupAxis::Repo
                | GroupAxis::Cycle => BTreeMap::new(),
            };
            for item in filtered {
                let (key, label) = axis.key_label(&item, kinds);
                let entry = buckets.entry(key).or_insert_with(|| (label, Vec::new()));
                entry.1.push(item);
            }
            buckets
                .into_iter()
                .map(|(k, (label, items))| (k, label, items))
                .collect()
        }
    };

    for (_, _, items) in &mut groups {
        sort_items(items, &keys);
    }

    // limit truncates the flattened sequence in group order
    if let Some(limit) = spec.limit.map(|l| l as usize) {
        let mut remaining = limit;
        for (_, _, items) in &mut groups {
            let take = remaining.min(items.len());
            items.truncate(take);
            remaining -= take;
        }
    }

    groups
        .into_iter()
        .map(|(_, label, items)| ViewGroup {
            label: if spec.effective_group_by().is_some() {
                Some(label)
            } else {
                None
            },
            items,
        })
        .collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::ItemId;

    fn t(n: u32, title: &str) -> Item {
        Item::new(ItemId::new(ItemKind::Task, n), title)
    }

    fn table() -> KindTable {
        KindTable::prelude()
    }

    fn items() -> Vec<Item> {
        vec![
            t(1, "a").priority(Priority::Low).epic("v2"),
            t(2, "b").priority(Priority::Critical),
            t(3, "c")
                .status(Status::InProgress)
                .owner("devin".parse().unwrap()),
            t(4, "d").status(Status::Done),
            Item::new(ItemId::new(ItemKind::Issue, 1), "e")
                .priority(Priority::High)
                .label("bug"),
        ]
    }

    fn flat_ids(groups: &[ViewGroup]) -> Vec<String> {
        groups
            .iter()
            .flat_map(|g| g.items.iter())
            .map(|i| i.id.to_string())
            .collect()
    }

    #[test]
    fn default_spec_passes_all_in_pri_order() {
        let v = items();
        let g = eval(&v, &ViewSpec::default(), &table());
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].label, None);
        // crit, high, then the two meds by id, then low
        assert_eq!(flat_ids(&g), ["T-002", "I-001", "T-003", "T-004", "T-001"]);
    }

    #[test]
    fn filter_is_and_across_or_within() {
        let v = items();
        let spec: ViewSpec =
            toml::from_str(r#"filter = { status = ["open", "in-progress"], pri_min = "high" }"#)
                .unwrap();
        spec.validate().unwrap();
        assert_eq!(flat_ids(&eval(&v, &spec, &table())), ["T-002", "I-001"]);
    }

    #[test]
    fn claimed_false_selects_unowned() {
        let v = items();
        let spec: ViewSpec = toml::from_str(r#"filter = { claimed = false }"#).unwrap();
        assert_eq!(
            flat_ids(&eval(&v, &spec, &table())),
            ["T-002", "I-001", "T-004", "T-001"]
        );
        // claimed + owner is a spec error, not a silent empty set
        let bad: ViewSpec =
            toml::from_str(r#"filter = { claimed = false, owner = ["devin"] }"#).unwrap();
        assert!(bad.validate().is_err());
    }

    #[test]
    fn pri_bounds_validate_order() {
        let spec: ViewSpec =
            toml::from_str(r#"filter = { pri_min = "high", pri_max = "med" }"#).unwrap();
        assert!(spec.validate().is_err());
        let ok: ViewSpec =
            toml::from_str(r#"filter = { pri_min = "med", pri_max = "crit" }"#).unwrap();
        ok.validate().unwrap();
    }

    #[test]
    fn group_by_status_yields_all_columns() {
        let v = items();
        let spec: ViewSpec = toml::from_str(r#"group_by = "status""#).unwrap();
        let g = eval(&v, &spec, &table());
        let labels: Vec<_> = g.iter().map(|g| g.label.as_deref().unwrap()).collect();
        assert_eq!(
            labels,
            [
                "open",
                "claimed",
                "in-progress",
                "blocked",
                "done",
                "deferred",
                "cancelled"
            ]
        );
        assert_eq!(g[0].items.len(), 3); // open: T-1, T-2, I-1
        assert_eq!(g[2].items.len(), 1); // in-progress
        assert_eq!(g[4].items.len(), 1); // done
    }

    #[test]
    fn named_axis_buckets_and_ungrouped_last() {
        let v = items();
        let spec: ViewSpec = toml::from_str(r#"group_by = "epic""#).unwrap();
        let g = eval(&v, &spec, &table());
        let labels: Vec<_> = g.iter().map(|g| g.label.as_deref().unwrap()).collect();
        assert_eq!(labels, ["v2", UNGROUPED]);
        assert_eq!(g[1].items.len(), 4);
    }

    #[test]
    fn filter_matches_scheduling_fields() {
        let mut v = items();
        v[0] = v[0]
            .clone()
            .due(crate::date::parse("2026-12-01").unwrap())
            .estimate(3)
            .cycle("sprint-12");
        let spec: ViewSpec =
            toml::from_str(r#"filter = { due = "2026-12-01", estimate = 3, cycle = "sprint-12" }"#)
                .unwrap();
        assert_eq!(flat_ids(&eval(&v, &spec, &table())), ["T-001"]);
        // a set field that doesn't match excludes the item
        let spec: ViewSpec = toml::from_str(r#"filter = { cycle = "sprint-13" }"#).unwrap();
        assert!(flat_ids(&eval(&v, &spec, &table())).is_empty());
        let spec: ViewSpec = toml::from_str(r#"filter = { estimate = 8 }"#).unwrap();
        assert!(flat_ids(&eval(&v, &spec, &table())).is_empty());
        let spec: ViewSpec = toml::from_str(r#"filter = { due = "2026-12-02" }"#).unwrap();
        assert!(flat_ids(&eval(&v, &spec, &table())).is_empty());
        // malformed dates are toml errors, not silent non-matches
        assert!(toml::from_str::<ViewSpec>(r#"filter = { due = "soon" }"#).is_err());
    }

    #[test]
    fn sort_by_scheduling_fields() {
        let mut v = items();
        v[0] = v[0]
            .clone()
            .due(crate::date::parse("2026-12-02").unwrap())
            .estimate(8)
            .cycle("b");
        v[1] = v[1]
            .clone()
            .due(crate::date::parse("2026-12-01").unwrap())
            .estimate(3)
            .cycle("a");
        // due asc — dated first (2026-12-01 then -02), rest trail
        let spec: ViewSpec = toml::from_str(r#"sort = ["due"]"#).unwrap();
        assert_eq!(
            flat_ids(&eval(&v, &spec, &table()))[..2],
            ["T-002", "T-001"]
        );
        let spec: ViewSpec = toml::from_str(r#"sort = ["estimate"]"#).unwrap();
        assert_eq!(
            flat_ids(&eval(&v, &spec, &table()))[..2],
            ["T-002", "T-001"]
        );
        // `est` is the short alias; display emits the canonical name
        assert_eq!("est".parse::<SortKey>().unwrap().field, SortField::Estimate);
        assert_eq!(
            SortKey {
                field: SortField::Estimate,
                desc: false
            }
            .to_string(),
            "estimate"
        );
        let spec: ViewSpec = toml::from_str(r#"sort = ["-cycle"]"#).unwrap();
        assert_eq!(
            flat_ids(&eval(&v, &spec, &table()))[..2],
            ["T-001", "T-002"]
        );
    }

    #[test]
    fn sort_desc_and_none_last() {
        let mut v = items();
        v[0] = v[0]
            .clone()
            .created(crate::date::parse("2026-01-01").unwrap());
        v[2] = v[2]
            .clone()
            .created(crate::date::parse("2026-03-01").unwrap());
        let spec: ViewSpec = toml::from_str(r#"sort = ["-created"]"#).unwrap();
        assert_eq!(
            flat_ids(&eval(&v, &spec, &table())),
            ["T-003", "T-001", "T-002", "T-004", "I-001"]
        );
        // ascending: created asc, but absent dates trail
        let spec: ViewSpec = toml::from_str(r#"sort = ["created"]"#).unwrap();
        assert_eq!(
            flat_ids(&eval(&v, &spec, &table())),
            ["T-001", "T-003", "T-002", "T-004", "I-001"]
        );
    }

    #[test]
    fn limit_truncates_across_groups() {
        let v = items();
        let spec: ViewSpec = toml::from_str("group_by = \"status\"\nlimit = 2\n").unwrap();
        let g = eval(&v, &spec, &table());
        // open bucket sorted crit,high then cut
        assert_eq!(flat_ids(&g), ["T-002", "I-001"]);
    }

    #[test]
    fn board_defaults_axis_to_status() {
        let spec: ViewSpec = toml::from_str(r#"format = "board""#).unwrap();
        assert_eq!(spec.effective_group_by(), Some(GroupAxis::Status));
    }

    #[test]
    fn repos_format_parses_names_and_needs_no_axis() {
        let spec: ViewSpec = toml::from_str(
            r#"source = "index"
format = "repos""#,
        )
        .unwrap();
        assert_eq!(spec.format, ViewFormat::Repos);
        assert_eq!(spec.format.name(), "repos");
        // repos emits repo rows — no implicit group axis
        assert_eq!(spec.effective_group_by(), None);
        assert_eq!("repos".parse::<ViewFormat>().unwrap(), ViewFormat::Repos);
        assert_eq!(
            "wat".parse::<ViewFormat>().unwrap_err().to_string(),
            "invalid format \"wat\" — expected list|board|repos|timeline|stats|burnup"
        );
        // serde round-trips through the lowercase name
        assert!(toml::to_string(&spec).unwrap().contains("repos"));
    }

    #[test]
    fn sort_key_parse_and_display() {
        assert!("-created".parse::<SortKey>().unwrap().desc);
        assert_eq!("pri".parse::<SortKey>().unwrap().field, SortField::Pri);
        assert!("wat".parse::<SortKey>().is_err());
        assert!("-".parse::<SortKey>().is_err());
        assert_eq!(
            SortKey {
                field: SortField::Id,
                desc: true
            }
            .to_string(),
            "-id"
        );
    }

    #[test]
    fn spec_toml_roundtrip() {
        let text = r##"
filter = { kind = ["issue"], status = ["open"], pri_min = "high", label = ["bug"], claimed = false }
group_by = "epic"
sort = ["-created", "pri"]
limit = 10
format = "board"
"##;
        let spec: ViewSpec = toml::from_str(text).unwrap();
        spec.validate().unwrap();
        assert_eq!(spec.limit, Some(10));
        assert_eq!(spec.format, ViewFormat::Board);
        assert_eq!(spec.filter.pri_min, Some(Priority::High));
        let back: ViewSpec = toml::from_str(&toml::to_string(&spec).unwrap()).unwrap();
        assert_eq!(back, spec);
    }

    #[test]
    fn priority_short_tokens_parse() {
        let spec: ViewSpec =
            toml::from_str(r#"filter = { pri_min = "med", pri_max = "crit" }"#).unwrap();
        assert_eq!(spec.filter.pri_min, Some(Priority::Medium));
        assert_eq!(spec.filter.pri_max, Some(Priority::Critical));
    }

    #[test]
    fn source_parses_and_skips_default() {
        let spec: ViewSpec = toml::from_str(r#"source = "index""#).unwrap();
        assert_eq!(spec.source, ViewSource::Index);
        assert!(toml::to_string(&spec).unwrap().contains("index"));
        // repo is the default — never emitted
        let spec = ViewSpec::default();
        assert_eq!(spec.source, ViewSource::Repo);
        assert!(!toml::to_string(&spec).unwrap().contains("source"));
        assert!("index".parse::<ViewSource>().unwrap() == ViewSource::Index);
        assert!("wat".parse::<ViewSource>().is_err());
    }

    #[test]
    fn repo_axis_groups_by_tag() {
        let v = items();
        let spec: ViewSpec = toml::from_str(r#"group_by = "repo""#).unwrap();
        let tagged = v
            .iter()
            .enumerate()
            .map(|(i, it)| Scoped::tagged(if i % 2 == 0 { "a" } else { "b" }, it));
        let g = eval_scoped(tagged, &spec, &table());
        let labels: Vec<_> = g.iter().map(|g| g.label.as_deref().unwrap()).collect();
        assert_eq!(labels, ["a", "b"]);
        assert_eq!(g[0].items.len(), 3);
        // tags survive eval — renderers get the repo prefix
        assert_eq!(g[0].items[0].repo, Some("a"));
        // a repo-source universe (untagged) groups under `ungrouped`
        let g = eval(&v, &spec, &table());
        let labels: Vec<_> = g.iter().map(|g| g.label.as_deref().unwrap()).collect();
        assert_eq!(labels, [UNGROUPED]);
        assert_eq!(g[0].items.len(), 5);
    }

    #[test]
    fn scoped_serializes_the_item_plus_repo() {
        let v = items();
        let untagged = serde_json::to_value(Scoped::new(&v[0])).unwrap();
        assert!(untagged.get("repo").is_none());
        assert_eq!(untagged["id"], "T-001");
        let tagged = serde_json::to_value(Scoped::tagged("myco", &v[0])).unwrap();
        assert_eq!(tagged["repo"], "myco");
        assert_eq!(tagged["id"], "T-001");
    }
}
