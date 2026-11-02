//! `myco board [--md]` — kanban of the current repo's ledger:
//! Backlog | Open | Claimed | In Progress | Blocked columns (done
//! items leave the ledger, so there's no Done column). Human
//! output is a `tabled` grid; `--md` is the same board as a
//! markdown table for embedding in docs; `--json` is the columns
//! map.
//!
//! The verb resolves the `board` named view — a repo's
//! `[views.board]` config shadows the built-in preset — and
//! evaluates it through `myco_core::eval`. The classic
//! status-columns spec keeps the legacy chrome described above; a
//! shadowed spec renders through the generic view renderers.

use std::collections::BTreeSet;
use std::path::PathBuf;

use myco_core::{
    eval, GroupAxis, Item, ItemId, Status, ViewFilter, ViewFormat, ViewGroup, ViewSpec,
};
use serde_json::json;
use tabled::builder::Builder;
use tabled::settings::Style;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::{digest, items, ops, view};

/// Default cap on board rows for human output (`--all` lifts it) —
/// a screenful, so a huge ledger can't bury the terminal.
pub const DEFAULT_MAX_ROWS: usize = 25;

/// The ascii style's per-table overhead: `| ` + ` ` per column
/// (3n) plus the outer borders (1).
fn table_overhead(n_cols: usize) -> usize {
    3 * n_cols + 1
}

/// Human-render constraints for the board — width budget + row
/// cap. `--md`/`--json` use [`Fit::UNBOUNDED`]; caps are a terminal
/// concern only.
#[derive(Debug, Clone, Copy)]
pub struct Fit {
    /// Total table width budget in terminal columns.
    pub width: usize,
    /// Max data rows before a `… +N more` footer.
    pub max_rows: usize,
}

impl Fit {
    /// No constraints — markdown + JSON output.
    pub const UNBOUNDED: Fit = Fit {
        width: usize::MAX,
        max_rows: usize::MAX,
    };

    /// Fit for human output — `all` lifts the row cap.
    pub fn terminal(all: bool) -> Fit {
        Fit {
            width: term_width(),
            max_rows: if all { usize::MAX } else { DEFAULT_MAX_ROWS },
        }
    }
}

/// Display width budget: `$MYCO_WIDTH` → tty width → 100 piped.
fn term_width() -> usize {
    if let Some(w) = std::env::var("MYCO_WIDTH")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
    {
        return w;
    }
    terminal_size::terminal_size()
        .map(|(w, _)| usize::from(w.0))
        .unwrap_or(100)
}

/// ANSI on? `NO_COLOR` kills it, `CLICOLOR_FORCE`/`FORCE_COLOR`
/// force it, else only on a tty.
fn color_enabled() -> bool {
    use std::io::IsTerminal;
    if std::env::var_os("NO_COLOR").is_some() {
        return false;
    }
    if std::env::var("CLICOLOR_FORCE")
        .or_else(|_| std::env::var("FORCE_COLOR"))
        .is_ok_and(|v| v != "0")
    {
        return true;
    }
    std::io::stdout().is_terminal()
}

/// `\x1b[<code>m…\x1b[0m` wrapper — raw escapes, no dep.
fn sgr(code: &str, s: &str) -> String {
    format!("\x1b[{code}m{s}\x1b[0m")
}

fn dim(s: &str) -> String {
    sgr("2", s)
}

/// Per-lane header tints — parked lanes dim, active lanes bright.
const HEADER_STYLE: [(&str, &str); 5] = [
    ("Backlog", "2"),
    ("Open", "1;36"),
    ("Claimed", "1;33"),
    ("In Progress", "1;32"),
    ("Blocked", "1;31"),
];

/// `T-008` / `thal:T-118` / `ADR-0001` — the leading card token.
fn looks_like_id(tok: &str) -> bool {
    let id = tok.rsplit(':').next().unwrap_or(tok);
    let mut parts = id.split('-');
    matches!(parts.next(), Some(k) if !k.is_empty() && k.chars().all(|c| c.is_ascii_uppercase()))
        && matches!(parts.next(), Some(n) if !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
}

/// A header cell (` Name `) gets its lane tint, padding kept.
fn style_header_cell(cell: &str) -> String {
    let name = cell.trim();
    if let Some((_, code)) = HEADER_STYLE.iter().find(|(n, _)| *n == name) {
        let lead = &cell[..cell.len() - cell.trim_start().len()];
        let trail = &cell[cell.trim_end().len()..];
        return format!("{lead}{}{trail}", sgr(code, name));
    }
    cell.to_string()
}

/// A card cell gets its leading item id bolded.
fn style_card_cell(cell: &str) -> String {
    let lead_len = cell.len() - cell.trim_start().len();
    let trail_start = cell.trim_end().len();
    if lead_len >= trail_start {
        return cell.to_string();
    }
    let body = &cell[lead_len..trail_start];
    let id_len = body.find(' ').unwrap_or(body.len());
    let (id, rest) = body.split_at(id_len);
    if !looks_like_id(id) {
        return cell.to_string();
    }
    format!(
        "{}{}{rest}{}",
        &cell[..lead_len],
        sgr("1", id),
        &cell[trail_start..]
    )
}

/// Paint a rendered board: dim the `+-|` chrome so content pops,
/// tint the header lane names, bold card ids, dim the footer.
fn colorize(out: &str) -> String {
    let pipe = dim("|");
    let mut header_done = false;
    let mut lines = Vec::new();
    for line in out.split('\n') {
        if line.is_empty() {
            lines.push(String::new());
        } else if line.starts_with('+') {
            lines.push(dim(line));
        } else if line.starts_with('|') {
            let styled: Vec<String> = line
                .split('|')
                .map(|cell| {
                    if header_done {
                        style_card_cell(cell)
                    } else {
                        style_header_cell(cell)
                    }
                })
                .collect();
            header_done = true;
            lines.push(styled.join(&pipe));
        } else {
            lines.push(dim(line));
        }
    }
    lines.join("\n")
}

/// `…`-truncate `s` to `w` display columns.
fn truncate(s: &str, w: usize) -> String {
    if UnicodeWidthStr::width(s) <= w {
        return s.to_string();
    }
    let mut out = String::new();
    let mut used = 0;
    for ch in s.chars() {
        let cw = UnicodeWidthChar::width(ch).unwrap_or(0);
        if used + cw > w.saturating_sub(1) {
            break;
        }
        out.push(ch);
        used += cw;
    }
    out.push('…');
    out
}

/// Water-fill `budget` across N columns: a column under the fair
/// share keeps its natural width and the rest split what's left.
/// Never grows a column past its natural width.
fn fit_widths_dyn(mut out: Vec<usize>, budget: usize) -> Vec<usize> {
    let mut locked = vec![false; out.len()];
    let mut open = out.len();
    let mut rem = budget;
    loop {
        if open == 0 {
            break;
        }
        let share = rem / open;
        let mut extra = rem - share * open;
        let mut progressed = false;
        for c in 0..out.len() {
            if !locked[c] && out[c] <= share {
                locked[c] = true;
                open -= 1;
                rem -= out[c];
                progressed = true;
            }
        }
        if !progressed {
            for c in 0..out.len() {
                if !locked[c] {
                    // every open column wants more than the share
                    out[c] = share + usize::from(extra > 0);
                    extra = extra.saturating_sub(1);
                    locked[c] = true;
                    open -= 1;
                }
            }
        }
    }
    out
}

/// Column order — a `deferred` item is a backlog item wherever it
/// lives, and `done` has no column (done leaves the ledger).
pub const COLUMNS: [(&str, Option<Status>); 5] = [
    ("backlog", Some(Status::Deferred)),
    ("open", Some(Status::Open)),
    ("claimed", Some(Status::Claimed)),
    ("in-progress", Some(Status::InProgress)),
    ("blocked", Some(Status::Blocked)),
];

/// Card text — `T-012 the title`, with an optional prefix
/// (`myco:`) for org boards.
pub(crate) fn card(item: &Item, prefix: Option<&str>, interrupted: bool) -> String {
    let id = match prefix {
        Some(p) => format!("{p}:{}", item.id),
        None => item.id.to_string(),
    };
    let id = if interrupted { format!("{id} ⚡") } else { id };
    if item.title.is_empty() {
        id
    } else {
        format!("{id} {}", item.title)
    }
}

/// The lane spec — the five lane statuses grouped by status;
/// sort and limit take the engine defaults (`pri,id`, none). It
/// matches the built-in `board` view's selection, so [`columns`]
/// and [`columns_json`] keep their exact legacy bucketing while
/// going through `eval`.
fn lane_spec() -> ViewSpec {
    ViewSpec {
        filter: ViewFilter {
            status: COLUMNS.iter().filter_map(|(_, s)| *s).collect(),
            ..ViewFilter::default()
        },
        group_by: Some(GroupAxis::Status),
        ..ViewSpec::default()
    }
}

/// The lane a status-token group label maps to, if any —
/// `deferred` leads as `backlog`; `done` has no lane.
fn lane_index(label: Option<&str>) -> Option<usize> {
    COLUMNS
        .iter()
        .position(|(_, s)| s.map(Status::token) == label)
}

/// Is `spec` the classic status-columns board? Then the legacy
/// chrome applies (capitalized lanes, backlog-first, no Done
/// column); a config-shadowed spec renders through the generic
/// view renderers instead.
pub(crate) fn is_status_board(spec: &ViewSpec) -> bool {
    spec.format == ViewFormat::Board && spec.effective_group_by() == Some(GroupAxis::Status)
}

/// `(header, cards)` pairs for a status-grouped eval result —
/// the five lanes in legacy order headed by their chrome titles,
/// then any extra non-empty bucket the spec admitted (a config
/// `board` view matching `done` earns a `done` column).
fn status_cards(
    groups: &[ViewGroup],
    prefix: Option<&str>,
    interrupted: &BTreeSet<ItemId>,
) -> (Vec<String>, Vec<Vec<String>>) {
    let card_col = |items: &[&Item]| {
        items
            .iter()
            .map(|&i| card(i, prefix, interrupted.contains(&i.id)))
            .collect::<Vec<String>>()
    };
    let mut headers = Vec::new();
    let mut columns = Vec::new();
    for (i, (title, _)) in HEADER_STYLE.iter().enumerate() {
        headers.push((*title).to_string());
        columns.push(
            groups
                .iter()
                .find(|g| lane_index(g.label.as_deref()) == Some(i))
                .map(|g| card_col(&g.items))
                .unwrap_or_default(),
        );
    }
    for g in groups {
        if lane_index(g.label.as_deref()).is_none() && !g.items.is_empty() {
            headers.push(g.label.clone().unwrap_or_default());
            columns.push(card_col(&g.items));
        }
    }
    (headers, columns)
}

/// Render N columns of card strings — the five-column board and
/// `myco view`'s generic columnar format share this. `headers[c]`
/// heads `columns[c]`; the same width-fitting, row cap, and chrome
/// apply at any column count.
pub fn render_columns(headers: &[String], columns: &[Vec<String>], md: bool, fit: Fit) -> String {
    let fit = if md { Fit::UNBOUNDED } else { fit };
    // shrink cards to the terminal budget — wide columns split
    // what's left once narrow ones keep their natural width
    let natural: Vec<usize> = headers
        .iter()
        .enumerate()
        .map(|(c, h)| {
            columns[c]
                .iter()
                .map(|s| UnicodeWidthStr::width(s.as_str()))
                .max()
                .unwrap_or(0)
                .max(UnicodeWidthStr::width(h.as_str()))
        })
        .collect();
    let budget = fit.width.saturating_sub(table_overhead(headers.len()));
    let widths = fit_widths_dyn(natural, budget);
    let height = columns.iter().map(Vec::len).max().unwrap_or(0);
    let shown = height.min(fit.max_rows);
    let mut builder = Builder::default();
    builder.push_record(headers.iter().map(String::as_str));
    for r in 0..shown {
        builder.push_record(headers.iter().enumerate().map(|(c, _)| {
            columns[c]
                .get(r)
                .map(|s| truncate(s, widths[c]))
                .unwrap_or_default()
        }));
    }
    let mut table = builder.build();
    if md {
        table.with(Style::markdown());
    }
    let mut out = table.to_string();
    let hidden: usize = columns.iter().map(|c| c.len().saturating_sub(shown)).sum();
    if hidden > 0 {
        out.push_str(&format!("\n… +{hidden} more (`--all` to show)"));
    }
    if !md && color_enabled() {
        out = colorize(&out);
    }
    out
}

/// Render five columns of card strings — shared with `org board`,
/// which builds mixed-repo cards itself.
pub fn render_cards(cards: &[Vec<String>; 5], md: bool, fit: Fit) -> String {
    let headers: Vec<String> = ["Backlog", "Open", "Claimed", "In Progress", "Blocked"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    render_columns(&headers, cards, md, fit)
}

/// Bucket `(label, item)` pairs into the five columns, sorting
/// each by priority — org cards carry their repo prefix already.
pub fn cards_by_column<'a>(items: impl Iterator<Item = (String, &'a Item)>) -> [Vec<String>; 5] {
    let mut cols: [Vec<(&'a Item, String)>; 5] = Default::default();
    for (label, i) in items {
        if let Some(idx) = COLUMNS.iter().position(|(_, s)| *s == Some(i.status)) {
            cols[idx].push((i, label));
        }
    }
    std::array::from_fn(|c| {
        let mut col = std::mem::take(&mut cols[c]);
        col.sort_by(|(a, _), (b, _)| digest::pri_then_id(a, b));
        col.into_iter().map(|(_, l)| l).collect()
    })
}

/// Serialize one column's items — `prefix` tags each item `repo`
/// (org boards); `interrupted` adds the `"interrupted": bool`
/// flag (`None` omits it — org snapshots carry no handoffs).
fn items_json(
    items: &[&Item],
    prefix: Option<&str>,
    interrupted: Option<&BTreeSet<ItemId>>,
) -> Vec<serde_json::Value> {
    items
        .iter()
        .map(|it| {
            let mut v = serde_json::to_value(it).unwrap_or(json!(null));
            if let Some(obj) = v.as_object_mut() {
                if let Some(p) = prefix {
                    obj.insert("repo".into(), json!(p));
                }
                if let Some(set) = interrupted {
                    obj.insert("interrupted".into(), json!(set.contains(&it.id)));
                }
            }
            v
        })
        .collect()
}

/// The `--json` columns map — column name → full items. `prefix`
/// tags each item `repo` (org boards). `interrupted` — the set
/// whose latest handoff is an interrupt — marks each item
/// `"interrupted": bool`; `None` omits the flag (org board, where
/// snapshots carry no handoffs).
///
/// Bucketing goes through `eval` on the lane spec — identical to
/// the legacy shape, so `myco serve` and org boards keep the
/// five fixed keys (non-lane groups, e.g. `done`, stay dropped).
pub fn columns_json<'a>(
    items: impl Iterator<Item = &'a Item>,
    prefix: Option<&str>,
    interrupted: Option<&BTreeSet<myco_core::ItemId>>,
) -> serde_json::Value {
    let groups = eval(items, &lane_spec());
    let mut map = serde_json::Map::new();
    for g in &groups {
        if let Some(i) = lane_index(g.label.as_deref()) {
            map.insert(
                COLUMNS[i].0.to_string(),
                json!(items_json(&g.items, prefix, interrupted)),
            );
        }
    }
    json!(map)
}

/// Columns-map JSON from eval'd groups — the five lanes under
/// their legacy names, plus any extra non-empty bucket the spec
/// admitted (a shadowed `board` view matching `done` earns a
/// `done` key — the spec asked for it).
pub(crate) fn groups_json(
    groups: &[ViewGroup],
    interrupted: &BTreeSet<ItemId>,
) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    for g in groups {
        match lane_index(g.label.as_deref()) {
            Some(i) => {
                map.insert(
                    COLUMNS[i].0.to_string(),
                    json!(items_json(&g.items, None, Some(interrupted))),
                );
            }
            None if !g.items.is_empty() => {
                if let Some(label) = &g.label {
                    map.insert(
                        label.clone(),
                        json!(items_json(&g.items, None, Some(interrupted))),
                    );
                }
            }
            None => {}
        }
    }
    json!(map)
}

/// An evaluated view ready to emit — the shared tail of the
/// view-backed board verbs (`board`, `epic <name>`,
/// `milestone <name>`).
pub(crate) struct Emit<'a> {
    /// Repo name for the nothing-matched message.
    pub repo: &'a str,
    /// View name for the generic `--json` payload.
    pub view: &'a str,
    /// The merged spec (named view + flag overrides).
    pub spec: &'a ViewSpec,
    /// The `eval` result.
    pub groups: &'a [ViewGroup<'a>],
    /// Interrupted-item marker set.
    pub interrupted: &'a BTreeSet<ItemId>,
    /// `--md` — markdown table.
    pub md: bool,
    /// `--all` — no row cap.
    pub all: bool,
    /// `--json` — machine output.
    pub json_out: bool,
    /// The verb's nothing-matched line.
    pub empty_msg: &'a str,
}

/// Emit one evaluated view. The classic status-board spec keeps
/// the legacy chrome + columns-map `--json`; a config-shadowed
/// spec renders through the generic view renderers and the
/// `myco view` payload shape.
pub(crate) fn emit(job: &Emit) -> Result<i32, String> {
    if job.json_out {
        if is_status_board(job.spec) {
            ops::print_json(&groups_json(job.groups, job.interrupted));
        } else {
            ops::print_json(&view::view_json(
                Some(job.view),
                job.spec,
                job.groups,
                job.interrupted,
            ));
        }
        return Ok(0);
    }
    if job.groups.iter().all(|g| g.items.is_empty()) {
        println!("{}: {}", job.repo, job.empty_msg);
        return Ok(0);
    }
    let rendered = if is_status_board(job.spec) {
        let (headers, cards) = status_cards(job.groups, None, job.interrupted);
        render_columns(&headers, &cards, job.md, Fit::terminal(job.all))
    } else {
        view::render_groups(job.spec, job.groups, job.interrupted, job.md, job.all)
    };
    ops::print_paged(&rendered);
    Ok(0)
}

/// The `board` verb — resolves the `board` named view (a repo's
/// `[views.board]` config shadows the preset) and folds
/// `--epic`/`--milestone` into its filter (both set = AND);
/// `--all` lifts the row cap.
pub fn run(
    dir: Option<PathBuf>,
    root: Option<PathBuf>,
    md: bool,
    all: bool,
    epic: Option<String>,
    milestone: Option<String>,
    json_out: bool,
) -> Result<i32, String> {
    let (proj, store) = items::open_read(dir.as_deref(), root.as_deref())?;
    let spec = view::scoped_spec(&proj.root, "board", epic, milestone)?;
    let scan = digest::scan(&store)?;
    // the flattened live set — nested children board as their own
    // cards, exactly like the legacy bucketing did
    let groups = eval(scan.live(), &spec);
    let interrupted = crate::continuity::interrupted_set(
        &store,
        groups.iter().flat_map(|g| g.items.iter().copied()),
    )?;
    emit(&Emit {
        repo: &scan.name,
        view: "board",
        spec: &spec,
        groups: &groups,
        interrupted: &interrupted,
        md,
        all,
        json_out,
        empty_msg: "empty board — `myco todo add` something",
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use myco_core::{ItemId, ItemKind, Priority};

    fn items() -> Vec<Item> {
        vec![
            Item::new(ItemId::new(ItemKind::Task, 1), "parked").status(Status::Deferred),
            Item::new(ItemId::new(ItemKind::Task, 2), "plain"),
            Item::new(ItemId::new(ItemKind::Task, 3), "urgent").priority(Priority::Critical),
            Item::new(ItemId::new(ItemKind::Issue, 1), "owned").status(Status::Claimed),
            Item::new(ItemId::new(ItemKind::Task, 4), "wip").status(Status::InProgress),
            Item::new(ItemId::new(ItemKind::Task, 5), "stuck").status(Status::Blocked),
            Item::new(ItemId::new(ItemKind::Task, 6), "gone").status(Status::Done),
        ]
    }

    /// Render through the same path `emit` takes for the classic
    /// spec — eval + lane chrome.
    fn render_lanes(items: &[Item], prefix: Option<&str>, md: bool, fit: Fit) -> String {
        let groups = eval(items.iter(), &lane_spec());
        let (headers, cards) = status_cards(&groups, prefix, &BTreeSet::new());
        render_columns(&headers, &cards, md, fit)
    }

    #[test]
    fn columns_bucket_and_sort() {
        let v = items();
        let groups = eval(v.iter(), &lane_spec());
        let col = |label: &str| {
            groups
                .iter()
                .find(|g| g.label.as_deref() == Some(label))
                .unwrap()
        };
        assert_eq!(col("deferred").items[0].id, ItemId::new(ItemKind::Task, 1));
        // open column: crit before med
        assert_eq!(
            col("open")
                .items
                .iter()
                .map(|i| i.id.number())
                .collect::<Vec<_>>(),
            [3, 2]
        );
        assert_eq!(col("claimed").items[0].id, ItemId::new(ItemKind::Issue, 1));
        assert_eq!(
            col("in-progress").items[0].id,
            ItemId::new(ItemKind::Task, 4)
        );
        assert_eq!(col("blocked").items[0].id, ItemId::new(ItemKind::Task, 5));
        // done items land nowhere
        assert!(groups
            .iter()
            .flat_map(|g| g.items.iter())
            .all(|i| i.status != Status::Done));
    }

    #[test]
    fn render_grid_and_md() {
        let v = items();
        let grid = render_lanes(&v, None, false, Fit::UNBOUNDED);
        assert!(grid.contains("Backlog"));
        assert!(grid.contains("In Progress"));
        assert!(grid.contains("T-004 wip"));
        let md = render_lanes(&v, Some("myco"), true, Fit::UNBOUNDED);
        assert!(md.starts_with('|'));
        assert!(md.contains("myco:T-001 parked"));
        assert!(md.contains("myco:I-001 owned"));
        // markdown table: header + separator + rows
        assert!(md.lines().any(|l| l.contains("---")));
    }

    #[test]
    fn fit_widths_water_fills() {
        // narrow columns keep natural width; the wide one gets
        // what's left of the budget
        assert_eq!(fit_widths_dyn(vec![500, 4, 4, 4, 4], 84), [68, 4, 4, 4, 4]);
        // everything fits → natural widths unchanged
        assert_eq!(
            fit_widths_dyn(vec![20, 10, 8, 11, 7], 100),
            [20, 10, 8, 11, 7]
        );
        // several wide columns split evenly (remainder to the first)
        assert_eq!(
            fit_widths_dyn(vec![30, 30, 30, 4, 4], 50),
            [14, 14, 14, 4, 4]
        );
        // never grown past natural
        assert_eq!(fit_widths_dyn(vec![80, 4, 4, 4, 4], 100), [80, 4, 4, 4, 4]);
    }

    #[test]
    fn truncate_fits_display_width() {
        assert_eq!(truncate("short", 10), "short");
        assert_eq!(truncate("T-148 Fix https://long-url", 10), "T-148 Fix…");
        assert_eq!(truncate("ab", 1), "…");
    }

    #[test]
    fn looks_like_id_covers_kinds_and_prefixes() {
        for id in ["T-008", "I-1", "P-001", "PL-12", "ADR-0001", "thal:T-118"] {
            assert!(looks_like_id(id), "{id}");
        }
        for not in ["", "task", "a-1", "T-x", "-1", "1-2"] {
            assert!(!looks_like_id(not), "{not}");
        }
    }

    #[test]
    fn colorize_dims_chrome_and_bolds_ids() {
        let v = [Item::new(ItemId::new(ItemKind::Task, 8), "work")];
        let plain = render_lanes(&v, None, false, Fit::UNBOUNDED);
        let out = colorize(&plain);
        // borders dimmed
        assert!(out.contains("\x1b[2m+"), "{out}");
        // header tinted
        assert!(out.contains("\x1b[1;36mOpen\x1b[0m"), "{out}");
        assert!(out.contains("\x1b[1;31mBlocked\x1b[0m"), "{out}");
        // card id bold
        assert!(out.contains("\x1b[1mT-008\x1b[0m"), "{out}");
        // content untouched
        assert!(out.contains("work"), "{out}");
    }

    #[test]
    fn render_caps_rows_and_width() {
        let mut v = Vec::new();
        for n in 1..=30 {
            v.push(
                Item::new(
                    ItemId::new(ItemKind::Task, n),
                    format!("a fairly long task title number {n}"),
                )
                .status(Status::Deferred),
            );
        }
        let fit = Fit {
            width: 60,
            max_rows: 25,
        };
        let out = render_lanes(&v, None, false, fit);
        assert!(out.contains("… +5 more"), "{out}");
        for line in out.lines() {
            assert!(
                UnicodeWidthStr::width(line) <= 60 + 5, // +header slack
                "{line}"
            );
        }
        // --all-equivalent: uncapped renders every row
        let full = render_lanes(&v, None, false, Fit::UNBOUNDED);
        assert!(full.contains("number 30"));
    }

    #[test]
    fn card_bare_id_for_empty_title() {
        let i = Item::new(ItemId::new(ItemKind::Task, 9), "");
        assert_eq!(card(&i, None, false), "T-009");
        assert_eq!(card(&i, Some("myco"), false), "myco:T-009");
    }

    #[test]
    fn columns_json_map() {
        let v = items();
        let j = columns_json(v.iter(), Some("myco"), None);
        assert_eq!(j["open"].as_array().unwrap().len(), 2);
        assert_eq!(j["open"][0]["id"], "T-003");
        assert_eq!(j["open"][0]["repo"], "myco");
        assert!(j["blocked"].as_array().unwrap()[0]["id"] == "T-005");
        assert!(j["backlog"].as_array().unwrap().len() == 1);
        // no interrupted set → no flag at all
        assert!(j["open"][0].get("interrupted").is_none());
        // with the set → per-item flag
        let set = BTreeSet::from([ItemId::new(ItemKind::Task, 3)]);
        let j = columns_json(v.iter(), None, Some(&set));
        assert_eq!(j["open"][0]["interrupted"], true);
        assert_eq!(j["open"][1]["interrupted"], false);
    }
}
