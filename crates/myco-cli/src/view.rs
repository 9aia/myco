//! `myco view [NAME]` — declarative views (P-006): named presets
//! from `[views]` config + built-ins, or an ad-hoc spec from
//! flags. One engine: filter → group → sort → limit → format.
//! Read-only — the write path stays `item set`/`todo`/`issue`.

use std::collections::BTreeSet;
use std::path::PathBuf;

use myco_core::{
    eval, eval_scoped, Actor, GroupAxis, Item, ItemId, ItemKind, Priority, Scoped, SortKey, Status,
    ViewFilter, ViewFormat, ViewGroup, ViewSource, ViewSpec,
};
use myco_ops::view::{NamedView, Origin};
use serde_json::json;

use crate::registry::Args;
use crate::{board, continuity, digest, indexcmd, items, ops, org};

/// The decoded filter flags — each maps onto the same
/// `ViewFilter` field `filter = { … }` uses. Shared by `view`
/// (merged over the named spec's filter) and `next` (merged over
/// its open-item default): a set flag replaces that field, unset
/// fields keep the base spec's value.
#[derive(Debug, Default)]
pub(crate) struct FilterPatch {
    pub(crate) kind: Vec<ItemKind>,
    pub(crate) status: Vec<Status>,
    pub(crate) pri_min: Option<Priority>,
    pub(crate) pri_max: Option<Priority>,
    pub(crate) label: Vec<String>,
    pub(crate) owner: Vec<Actor>,
    pub(crate) claimed: bool,
    pub(crate) unclaimed: bool,
    pub(crate) epic: Option<String>,
    pub(crate) milestone: Option<String>,
    pub(crate) live_only: bool,
    pub(crate) superseded: bool,
}

/// The filter args → the patch — `view` and `next` share the
/// flag set.
pub(crate) fn filter_args(a: &Args) -> Result<FilterPatch, String> {
    Ok(FilterPatch {
        kind: a.many_parsed("kind")?,
        status: a.many_parsed("status")?,
        pri_min: a.parsed("pri_min")?,
        pri_max: a.parsed("pri_max")?,
        label: a.many("label"),
        owner: a.many_parsed("owner")?,
        claimed: a.flag("claimed"),
        unclaimed: a.flag("unclaimed"),
        epic: a.opt("epic").map(str::to_string),
        milestone: a.opt("milestone").map(str::to_string),
        live_only: a.flag("live_only"),
        superseded: a.flag("superseded"),
    })
}

impl FilterPatch {
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

/// The ad-hoc spec patch — [`FilterPatch`] plus the
/// group/sort/limit/format spec fields; with NAME they merge
/// over the named spec (a set field replaces, unset keeps).
#[derive(Debug, Default)]
pub(crate) struct SpecPatchCli {
    pub(crate) filter: FilterPatch,
    pub(crate) source: Option<ViewSource>,
    pub(crate) group_by: Option<GroupAxis>,
    pub(crate) sort: Vec<SortKey>,
    pub(crate) limit: Option<u32>,
    pub(crate) format: Option<ViewFormat>,
}

/// `view`'s args → the spec patch.
pub(crate) fn spec_args(a: &Args) -> Result<SpecPatchCli, String> {
    Ok(SpecPatchCli {
        filter: filter_args(a)?,
        source: a.parsed("source")?,
        group_by: a.parsed("group_by")?,
        sort: a.many_parsed("sort")?,
        limit: a.parsed("limit")?,
        format: a.parsed("format")?,
    })
}

impl SpecPatchCli {
    /// Fold every set flag over `spec`; unset fields keep the
    /// named spec's value.
    fn apply(&self, spec: &mut ViewSpec) {
        self.filter.apply(&mut spec.filter);
        if let Some(s) = self.source {
            spec.source = s;
        }
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
            || self.source.is_some()
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
    if !s.source.is_default() {
        out.push_str(&format!(" source:{}", s.source.name()));
    }
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
/// groups; children ride indented under their parent. A
/// `repo:…` tag prefixes index-sourced items.
pub(crate) fn render_list(groups: &[ViewGroup], interrupted: &BTreeSet<ItemId>) -> String {
    fn push_item(
        out: &mut String,
        item: &Item,
        depth: usize,
        base: &str,
        repo: Option<&str>,
        interrupted: &BTreeSet<ItemId>,
    ) {
        out.push_str(base);
        out.push_str(&"  ".repeat(depth));
        if let Some(r) = repo {
            out.push_str(r);
            out.push(':');
        }
        out.push_str(&items::compact(item));
        if interrupted.contains(&item.id) {
            out.push_str(" ⚡");
        }
        out.push('\n');
        for c in &item.children {
            push_item(out, c, depth + 1, base, repo, interrupted);
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
            push_item(&mut out, i.item, 0, base, i.repo, interrupted);
        }
    }
    out.trim_end_matches('\n').to_string()
}

/// `board` — a status-grouped spec earns the lane chrome
/// (capitalized Backlog | Open | … lanes via
/// [`board::status_cards`]); any other grouping renders generic
/// columns headed by the group labels.
pub(crate) fn render_board(
    spec: &ViewSpec,
    groups: &[ViewGroup],
    interrupted: &BTreeSet<ItemId>,
    md: bool,
    all: bool,
) -> String {
    if board::is_status_board(spec) {
        let (headers, cards) = board::status_cards(groups, interrupted);
        return board::render_columns(&headers, &cards, md, board::Fit::terminal(all));
    }
    let headers: Vec<String> = groups
        .iter()
        .map(|g| g.label.clone().unwrap_or_default())
        .collect();
    let columns: Vec<Vec<String>> = groups
        .iter()
        .map(|g| {
            g.items
                .iter()
                .map(|i| board::card(i.item, i.repo, interrupted.contains(&i.item.id)))
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

/// Print resolution warnings to stderr — a config view shadowing
/// a built-in preset warns on every resolution.
pub(crate) fn warn(warnings: &[String]) {
    for w in warnings {
        eprintln!("myco: warning: {w}");
    }
}

/// Pick the renderer `spec.format` names — the shared tail of
/// every eval-driven read.
pub(crate) fn render_groups(
    spec: &ViewSpec,
    groups: &[ViewGroup],
    interrupted: &BTreeSet<ItemId>,
    md: bool,
    all: bool,
) -> String {
    match spec.format {
        ViewFormat::Board => render_board(spec, groups, interrupted, md, all),
        ViewFormat::List => render_list(groups, interrupted),
    }
}

/// The `--json` payload — `{view, spec, groups, interrupted}`.
/// A status-axis board takes the five-lane columns map
/// ([`board::groups_json`]) instead — the shape `myco board
/// --json` always emitted.
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

/// The index-sourced universe — every registered repo's
/// snapshot, freshness-checked + rebuilt by [`org::load_fresh`],
/// items tagged with their repo's name. Rescan failures warn on
/// stderr and contribute their last-good snapshot (or nothing).
pub(crate) fn index_universe() -> Result<(Vec<org::RepoView>, Vec<String>), String> {
    let index = indexcmd::open_index()?;
    let views = org::load_fresh(&index)?;
    let warnings = views
        .iter()
        .filter_map(|v| v.error.as_ref().map(|e| format!("{}: {e}", v.name)))
        .collect();
    Ok((views, warnings))
}

/// The `view` verb — `NAME` resolves builtin presets then config
/// views; no NAME builds the spec from flags alone; bare `myco
/// view` (or `--list`) enumerates the defined views.
/// `spec.source` picks the universe: `repo` reads this
/// worktree's ledger, `index` unions the registered repos'
/// snapshots (the `org-*` presets — `myco org status|board`'s
/// heirs).
/// The `view` verb — `NAME` resolves builtin presets then config
/// views; no NAME builds the spec from flags alone; bare `myco
/// view` (or `--list`) enumerates the defined views.
pub fn h(ctx: &crate::registry::Ctx, a: &Args) -> Result<i32, String> {
    let name = a.opt("name").map(str::to_string);
    let flags = spec_args(a)?;
    let out = Out {
        list: a.flag("list"),
        md: a.flag("md"),
        all: a.flag("all"),
    };
    run(
        name,
        flags,
        out,
        ctx.dir.clone(),
        ctx.root.clone(),
        ctx.json,
    )
}

fn run(
    name: Option<String>,
    flags: SpecPatchCli,
    out: Out,
    dir: Option<PathBuf>,
    root: Option<PathBuf>,
    json_out: bool,
) -> Result<i32, String> {
    // The config root for named-view resolution — repo discovery
    // when it works, else `--root`/cwd: an index-sourced view
    // needs no store, so discovery failure isn't fatal (yet).
    let opened = items::open_read(dir.as_deref(), root.as_deref());
    let cfg_root = opened
        .as_ref()
        .map(|(p, _)| p.root.clone())
        .ok()
        .or_else(|| root.clone())
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));

    if out.list || (name.is_none() && !flags.any_set()) {
        let (views, _) = myco_ops::view::named_views_lenient(&cfg_root, ops::CONFIG)?;
        list_views(&views, json_out);
        return Ok(0);
    }

    let (mut spec, warnings) = match &name {
        Some(n) => {
            let (v, w) = myco_ops::view::resolve_lenient(&cfg_root, n, ops::CONFIG)?;
            (v.spec, w)
        }
        None => (ViewSpec::default(), Vec::new()),
    };
    flags.apply(&mut spec);
    spec.validate()?;
    warn(&warnings);

    // The universe owners — `groups` borrows item refs out of
    // them, so they must outlive it.
    let (interrupted, empty_name, scan, views) = match spec.source {
        ViewSource::Repo => {
            let (_proj, store) = opened?;
            let s = digest::scan(&store)?;
            (
                continuity::interrupted_set(&store, s.live())?,
                s.name.clone(),
                Some(s),
                Vec::new(),
            )
        }
        ViewSource::Index => {
            let (v, repo_warnings) = index_universe()?;
            warn(&repo_warnings);
            // snapshots carry no handoffs — the interrupted set
            // is empty, not failed
            (
                BTreeSet::new(),
                name.clone().unwrap_or_else(|| "index".into()),
                None,
                v,
            )
        }
    };
    // top-level items only — children ride with their parent
    // (they can't match a filter on their own)
    let groups = match spec.source {
        ViewSource::Repo => eval(
            scan.iter()
                .flat_map(|s| s.items.iter().filter(|i| !s.archived.contains(&i.id))),
            &spec,
        ),
        ViewSource::Index => eval_scoped(
            views.iter().flat_map(|v| {
                v.snap.iter().flat_map(|s| {
                    s.items
                        .iter()
                        .filter(|i| !s.archived.contains(&i.id))
                        .map(move |i| Scoped::tagged(s.name.as_str(), i))
                })
            }),
            &spec,
        ),
    };

    if json_out {
        let payload = if board::is_status_board(&spec) {
            // index snapshots carry no handoffs — omit the flag
            // entirely rather than emitting `false` everywhere
            let set = (spec.source == ViewSource::Repo).then_some(&interrupted);
            board::groups_json(&groups, set)
        } else {
            view_json(name.as_deref(), &spec, &groups, &interrupted)
        };
        ops::print_json(&payload);
        return Ok(0);
    }

    let rendered = render_groups(&spec, &groups, &interrupted, out.md, out.all);
    if groups.iter().all(|g| g.items.is_empty()) {
        println!("{empty_name}: empty view — no items match");
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
    fn repo_config_shadow_redefines_the_named_board() {
        // `[views.board]` in the repo's myco.toml replaces the
        // preset `myco view board` resolves — the spec-level proof
        // that config shadowing reaches the named view, and that a
        // non-status spec drops the lane chrome.
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
                let (v, warnings) =
                    myco_ops::view::resolve_lenient(dir.path(), "board", ops::CONFIG).unwrap();
                assert_eq!(warnings.len(), 1);
                let spec = v.spec;
                assert_eq!(spec.filter.kind, vec![ItemKind::Issue]);
                // the shadowed spec isn't a status-axis board →
                // the generic renderers/json, not the lane chrome
                assert!(!board::is_status_board(&spec));
                // flag merge still applies over the shadowed spec
                let mut scoped = spec.clone();
                SpecPatchCli {
                    filter: FilterPatch {
                        epic: Some("v2".into()),
                        ..Default::default()
                    },
                    ..Default::default()
                }
                .apply(&mut scoped);
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
