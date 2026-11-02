//! `myco view [NAME]` — declarative views (P-006): named presets
//! from `[views]` config + built-ins, or an ad-hoc spec from
//! flags. One engine: filter → group → sort → limit → format.
//! Read-only — the write path stays `item set`/`todo`/`issue`.

use std::collections::BTreeSet;
use std::path::PathBuf;

use clap::Args;
use myco_core::{
    eval, Actor, GroupAxis, Item, ItemId, ItemKind, Priority, SortKey, Status, ViewFilter,
    ViewFormat, ViewGroup, ViewSpec,
};
use myco_ops::view::{NamedView, Origin};
use serde_json::json;

use crate::{board, continuity, digest, items, ops};

/// The filter flags — each maps onto the same `ViewFilter`
/// field `filter = { … }` uses. Shared by `view` (merged over
/// the named spec's filter) and `next` (merged over its
/// open-item default): a set flag replaces that field, unset
/// fields keep the base spec's value.
#[derive(Debug, Default, Args)]
pub struct FilterFlags {
    /// Item kinds — task|issue|proposal|adr|plan
    /// (repeatable/comma-separated).
    #[arg(long, value_delimiter = ',')]
    pub(crate) kind: Vec<ItemKind>,
    /// Statuses — open|claimed|in-progress|blocked|done|deferred
    /// (repeatable/comma-separated).
    #[arg(long, value_delimiter = ',')]
    pub(crate) status: Vec<Status>,
    /// Inclusive lower severity bound — low|med|high|crit.
    #[arg(long)]
    pub(crate) pri_min: Option<Priority>,
    /// Inclusive upper severity bound.
    #[arg(long)]
    pub(crate) pri_max: Option<Priority>,
    /// `#`-less label — item carries any of these
    /// (repeatable/comma-separated).
    #[arg(long, value_delimiter = ',')]
    pub(crate) label: Vec<String>,
    /// Owner actor — item's owner is any of these
    /// (repeatable/comma-separated).
    #[arg(long, value_delimiter = ',')]
    pub(crate) owner: Vec<Actor>,
    /// Items with an owner.
    #[arg(long, conflicts_with = "unclaimed")]
    pub(crate) claimed: bool,
    /// Items with no owner.
    #[arg(long)]
    pub(crate) unclaimed: bool,
    /// Items in this epic.
    #[arg(long)]
    pub(crate) epic: Option<String>,
    /// Items in this milestone.
    #[arg(long)]
    pub(crate) milestone: Option<String>,
    /// Items not superseded by another (ADRs).
    #[arg(long, conflicts_with = "superseded")]
    pub(crate) live_only: bool,
    /// Items superseded by another (ADRs).
    #[arg(long)]
    pub(crate) superseded: bool,
}

impl FilterFlags {
    /// Fold every set flag into `f`; unset fields keep the
    /// base filter's value.
    pub(crate) fn apply(&self, f: &mut ViewFilter) {
        if !self.kind.is_empty() {
            f.kind = self.kind.clone();
        }
        if !self.status.is_empty() {
            f.status = self.status.clone();
        }
        if let Some(p) = self.pri_min {
            f.pri_min = Some(p);
        }
        if let Some(p) = self.pri_max {
            f.pri_max = Some(p);
        }
        if !self.label.is_empty() {
            f.label = self.label.clone();
        }
        if !self.owner.is_empty() {
            f.owner = self.owner.clone();
        }
        if self.claimed {
            f.claimed = Some(true);
        }
        if self.unclaimed {
            f.claimed = Some(false);
        }
        if let Some(e) = &self.epic {
            f.epic = Some(e.clone());
        }
        if let Some(m) = &self.milestone {
            f.milestone = Some(m.clone());
        }
        if self.live_only {
            f.superseded = Some(false);
        }
        if self.superseded {
            f.superseded = Some(true);
        }
    }

    /// Any flag set?
    pub(crate) fn any_set(&self) -> bool {
        !self.kind.is_empty()
            || !self.status.is_empty()
            || self.pri_min.is_some()
            || self.pri_max.is_some()
            || !self.label.is_empty()
            || !self.owner.is_empty()
            || self.claimed
            || self.unclaimed
            || self.epic.is_some()
            || self.milestone.is_some()
            || self.live_only
            || self.superseded
    }
}

/// The ad-hoc spec flags — [`FilterFlags`] plus the
/// group/sort/limit/format spec fields; with NAME they merge
/// over the named spec (a set flag replaces that field, unset
/// fields keep the named spec's value).
#[derive(Debug, Default, Args)]
pub struct ViewFlags {
    #[command(flatten)]
    pub(crate) filter: FilterFlags,
    /// Group axis — status|priority|kind|owner|epic|milestone.
    #[arg(long)]
    group_by: Option<GroupAxis>,
    /// Sort keys — `pri,id,-created`; `-` flips a key's default
    /// direction.
    #[arg(long, value_delimiter = ',')]
    sort: Vec<SortKey>,
    /// Keep the first N items after sorting.
    #[arg(long, value_name = "N")]
    limit: Option<u32>,
    /// Renderer — list|board.
    #[arg(long)]
    format: Option<ViewFormat>,
}

impl ViewFlags {
    /// Fold every set flag over `spec`; unset fields keep the
    /// named spec's value.
    fn apply(&self, spec: &mut ViewSpec) {
        self.filter.apply(&mut spec.filter);
        if let Some(g) = self.group_by {
            spec.group_by = Some(g);
        }
        if !self.sort.is_empty() {
            spec.sort = self.sort.clone();
        }
        if let Some(n) = self.limit {
            spec.limit = Some(n);
        }
        if let Some(fmt) = self.format {
            spec.format = fmt;
        }
    }

    /// Any flag set? Bare `myco view` (no name, no flags) lists.
    fn any_set(&self) -> bool {
        self.filter.any_set()
            || self.group_by.is_some()
            || !self.sort.is_empty()
            || self.limit.is_some()
            || self.format.is_some()
    }
}

/// One-line spec digest for `--list` — only set fields.
fn spec_digest(s: &ViewSpec) -> String {
    let f = &s.filter;
    let mut parts = Vec::new();
    let names = |v: &[ItemKind]| v.iter().map(|k| k.name()).collect::<Vec<_>>().join(",");
    if !f.kind.is_empty() {
        parts.push(format!("kind=[{}]", names(&f.kind)));
    }
    if !f.status.is_empty() {
        parts.push(format!(
            "status=[{}]",
            f.status
                .iter()
                .map(|s| s.token())
                .collect::<Vec<_>>()
                .join(",")
        ));
    }
    if let Some(p) = f.pri_min {
        parts.push(format!("pri_min={}", p.name()));
    }
    if let Some(p) = f.pri_max {
        parts.push(format!("pri_max={}", p.name()));
    }
    if !f.label.is_empty() {
        parts.push(format!("label=[{}]", f.label.join(",")));
    }
    if !f.owner.is_empty() {
        parts.push(format!(
            "owner=[{}]",
            f.owner
                .iter()
                .map(|a| a.as_str())
                .collect::<Vec<_>>()
                .join(",")
        ));
    }
    if let Some(c) = f.claimed {
        parts.push(format!("claimed={c}"));
    }
    if let Some(e) = &f.epic {
        parts.push(format!("epic={e}"));
    }
    if let Some(m) = &f.milestone {
        parts.push(format!("milestone={m}"));
    }
    if let Some(sup) = f.superseded {
        parts.push(format!("superseded={sup}"));
    }
    let mut out = if parts.is_empty() {
        "filter:{}".to_string()
    } else {
        format!("filter:{{{}}}", parts.join(" "))
    };
    if let Some(g) = s.group_by {
        out.push_str(&format!(" group_by:{}", g.name()));
    }
    if !s.sort.is_empty() {
        out.push_str(&format!(
            " sort:{}",
            s.sort
                .iter()
                .map(|k| k.to_string())
                .collect::<Vec<_>>()
                .join(",")
        ));
    }
    if let Some(n) = s.limit {
        out.push_str(&format!(" limit:{n}"));
    }
    out.push_str(&format!(" format:{}", s.format.name()));
    out
}

/// The `--list` output.
fn list_views(views: &[NamedView], json_out: bool) {
    if json_out {
        ops::print_json(&json!({
            "views": views.iter().map(|v| json!({
                "name": v.name,
                "origin": match v.origin {
                    Origin::Builtin => "builtin",
                    Origin::Config => "config",
                },
                "spec": v.spec,
            })).collect::<Vec<_>>(),
        }));
        return;
    }
    for v in views {
        let origin = match v.origin {
            Origin::Builtin => "builtin",
            Origin::Config => "config",
        };
        println!("{:<10} {:<7} {}", v.name, origin, spec_digest(&v.spec));
    }
}

/// `list` — compact lines; `<label> (n)` headers when the spec
/// groups; children ride indented under their parent.
pub(crate) fn render_list(groups: &[ViewGroup], interrupted: &BTreeSet<ItemId>) -> String {
    fn push_item(
        out: &mut String,
        item: &Item,
        depth: usize,
        base: &str,
        interrupted: &BTreeSet<ItemId>,
    ) {
        out.push_str(base);
        out.push_str(&"  ".repeat(depth));
        out.push_str(&items::compact(item));
        if interrupted.contains(&item.id) {
            out.push_str(" ⚡");
        }
        out.push('\n');
        for c in &item.children {
            push_item(out, c, depth + 1, base, interrupted);
        }
    }
    let mut out = String::new();
    for g in groups {
        if g.items.is_empty() {
            continue;
        }
        if let Some(label) = &g.label {
            out.push_str(&format!("{label} ({})\n", g.items.len()));
        }
        let base = if g.label.is_some() { "  " } else { "" };
        for i in &g.items {
            push_item(&mut out, i, 0, base, interrupted);
        }
    }
    out.trim_end_matches('\n').to_string()
}

/// `board` — group labels become column headers.
pub(crate) fn render_board(
    groups: &[ViewGroup],
    interrupted: &BTreeSet<ItemId>,
    md: bool,
    all: bool,
) -> String {
    let headers: Vec<String> = groups
        .iter()
        .map(|g| g.label.clone().unwrap_or_default())
        .collect();
    let columns: Vec<Vec<String>> = groups
        .iter()
        .map(|g| {
            g.items
                .iter()
                .map(|i| board::card(i, None, interrupted.contains(&i.id)))
                .collect()
        })
        .collect();
    board::render_columns(&headers, &columns, md, board::Fit::terminal(all))
}

/// Display toggles for the view verb.
#[derive(Debug, Default, Clone, Copy)]
pub struct Out {
    /// `--list` — enumerate defined views and exit.
    pub list: bool,
    /// `--md` — markdown table (board format).
    pub md: bool,
    /// `--all` — no row cap (board format).
    pub all: bool,
}

/// Print resolution warnings to stderr — every verb fronting a
/// named view reports a shadowed preset identically.
pub(crate) fn warn(warnings: &[String]) {
    for w in warnings {
        eprintln!("myco: warning: {w}");
    }
}

/// Resolve a named view and fold `--epic`/`--milestone`-style
/// scope overrides into its filter — the same merge
/// [`ViewFlags::apply`] performs (a set override wins, unset
/// fields keep the named spec). The merged spec is re-validated.
/// This is the front of the verb-fronted views (`board`,
/// `epic <name>`, `milestone <name>`): a repo's `[views.<name>]`
/// config shadows the built-in preset the same way it does for
/// `myco view`.
pub(crate) fn scoped_spec(
    root: &std::path::Path,
    name: &str,
    epic: Option<String>,
    milestone: Option<String>,
) -> Result<ViewSpec, String> {
    let (view, warnings) = myco_ops::view::resolve_lenient(root, name)?;
    warn(&warnings);
    let mut spec = view.spec;
    if let Some(e) = epic {
        spec.filter.epic = Some(e);
    }
    if let Some(m) = milestone {
        spec.filter.milestone = Some(m);
    }
    spec.validate()?;
    Ok(spec)
}

/// Pick the renderer `spec.format` names — the shared tail of
/// every eval-driven verb.
pub(crate) fn render_groups(
    spec: &ViewSpec,
    groups: &[ViewGroup],
    interrupted: &BTreeSet<ItemId>,
    md: bool,
    all: bool,
) -> String {
    match spec.format {
        ViewFormat::Board => render_board(groups, interrupted, md, all),
        ViewFormat::List => render_list(groups, interrupted),
    }
}

/// The `--json` payload — `myco view`'s shape, reused by the
/// verb-fronted views when a config-shadowed spec no longer fits
/// the legacy columns map.
pub(crate) fn view_json(
    name: Option<&str>,
    spec: &ViewSpec,
    groups: &[ViewGroup],
    interrupted: &BTreeSet<ItemId>,
) -> serde_json::Value {
    json!({
        "view": name,
        "spec": spec,
        "groups": groups.iter().map(|g| json!({
            "label": g.label,
            "items": g.items,
        })).collect::<Vec<_>>(),
        "interrupted": interrupted.iter().map(|i| i.to_string()).collect::<Vec<_>>(),
    })
}

/// The `view` verb — `NAME` resolves builtin presets then config
/// views; no NAME builds the spec from flags alone; bare `myco
/// view` (or `--list`) enumerates the defined views.
pub fn run(
    name: Option<String>,
    flags: ViewFlags,
    out: Out,
    dir: Option<PathBuf>,
    root: Option<PathBuf>,
    json_out: bool,
) -> Result<i32, String> {
    let (proj, store) = items::open_read(dir.as_deref(), root.as_deref())?;

    if out.list || (name.is_none() && !flags.any_set()) {
        let (views, _) = myco_ops::view::named_views(&proj.root)?;
        list_views(&views, json_out);
        return Ok(0);
    }

    let (mut spec, warnings) = match &name {
        Some(n) => {
            let (v, w) = myco_ops::view::resolve(&proj.root, n)?;
            (v.spec, w)
        }
        None => (ViewSpec::default(), Vec::new()),
    };
    flags.apply(&mut spec);
    spec.validate()?;
    warn(&warnings);

    // the universe: top-level items only — children ride with
    // their parent (they can't match a filter on their own)
    let scan = digest::scan(&store)?;
    let universe = scan.items.iter().filter(|i| !scan.archived.contains(&i.id));
    let groups = eval(universe, &spec);
    let interrupted = continuity::interrupted_set(&store, scan.live())?;

    if json_out {
        ops::print_json(&view_json(name.as_deref(), &spec, &groups, &interrupted));
        return Ok(0);
    }

    let rendered = render_groups(&spec, &groups, &interrupted, out.md, out.all);
    if groups.iter().all(|g| g.items.is_empty()) {
        println!("{}: empty view — no items match", scan.name);
    } else {
        ops::print_paged(&rendered);
    }
    Ok(0)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn scoped_spec_repo_config_shadows_the_verb() {
        // `[views.board]` in the repo's myco.toml replaces the
        // preset `myco board` (and `epic|milestone <name>`)
        // evaluate — the spec-level proof that config shadowing
        // reaches the read verbs, not just `myco view`.
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("myco.toml"),
            "[views.board]\nfilter = { kind = [\"issue\"] }\n",
        )
        .unwrap();
        temp_env::with_vars(
            [
                ("MYCO_CONFIG", None::<&str>),
                (
                    "HOME",
                    Some(dir.path().join("nohome").to_string_lossy().as_ref()),
                ),
                ("XDG_CONFIG_HOME", None::<&str>),
            ],
            || {
                let spec = scoped_spec(dir.path(), "board", None, None).unwrap();
                assert_eq!(spec.filter.kind, vec![ItemKind::Issue]);
                // flag merge still applies over the shadowed spec
                let scoped = scoped_spec(dir.path(), "board", Some("v2".into()), None).unwrap();
                assert_eq!(scoped.filter.epic.as_deref(), Some("v2"));
                // and eval honours the shadow — tasks drop out of
                // every group, so the rendered board changes
                let items = vec![
                    Item::new(ItemId::new(ItemKind::Task, 1), "t"),
                    Item::new(ItemId::new(ItemKind::Issue, 1), "i"),
                ];
                let ids: Vec<String> = eval(&items, &spec)
                    .iter()
                    .flat_map(|g| g.items.iter().map(|i| i.id.to_string()))
                    .collect();
                assert_eq!(ids, ["I-001"]);
            },
        );
    }
}
