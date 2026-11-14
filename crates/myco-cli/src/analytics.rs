//! The analytics emits — `format = "timeline"|"stats"|"burnup"`
//! (P-012). Same pipeline as every view: the spec's
//! `filter → group → sort → limit` resolves first; these formats
//! replace the *group render* with a purpose-built emit over the
//! filtered universe. Structured `--json` for pavo/agents,
//! deterministic ASCII on tty.
//!
//! Universe rules differ per emit:
//! - `timeline` — live top-level items (a roadmap is forward
//!   work; archived history has no lanes).
//! - `stats`/`burnup` — the whole ledger incl. DONE.md and
//!   `archive/` (the archive *is* their data), children flattened
//!   in — they're counts, not rows.
//!
//! `source = "index"` works identically: snapshots carry items
//! and the archived-id set, so repo/index parity holds for all
//! three emits.

use myco_core::analytics::{self, BurnupSeries, Lane, Stats};
use myco_core::view::{Scoped, ViewSpec};
use myco_core::{date, CycleConfig, KindTable, ViewFormat};
use serde_json::json;

use crate::digest::Scan;
use crate::org;

/// Does `spec.format` route to an analytics emit instead of the
/// group renderers?
pub(crate) fn is_emit(spec: &ViewSpec) -> bool {
    matches!(
        spec.format,
        ViewFormat::Timeline | ViewFormat::Stats | ViewFormat::Burnup
    )
}

/// The filtered, format-appropriate universe as [`Scoped`] items.
/// `scan`/`views` are the repo/index universe owners — exactly
/// one is populated per `spec.source`.
pub(crate) fn emit_items<'a>(
    spec: &ViewSpec,
    scan: Option<&'a Scan>,
    views: &'a [org::RepoView],
    kinds: &KindTable,
) -> Vec<Scoped<'a>> {
    // stats/burnup read the archive; timeline is forward work only
    let archived_ok = matches!(spec.format, ViewFormat::Stats | ViewFormat::Burnup);
    let mut out = Vec::new();
    match spec.source {
        myco_core::ViewSource::Repo => {
            if let Some(s) = scan {
                for top in &s.items {
                    if archived_ok {
                        for i in top.subtree() {
                            if spec.filter.matches(i, kinds) {
                                out.push(Scoped::new(i));
                            }
                        }
                    } else if !s.archived.contains(&top.id) && spec.filter.matches(top, kinds) {
                        out.push(Scoped::new(top));
                    }
                }
            }
        }
        myco_core::ViewSource::Index => {
            for v in views {
                if let Some(snap) = &v.snap {
                    for top in &snap.items {
                        if archived_ok {
                            for i in top.subtree() {
                                if spec.filter.matches(i, kinds) {
                                    out.push(Scoped::tagged(snap.name.as_str(), i));
                                }
                            }
                        } else if !snap.archived.contains(&top.id)
                            && spec.filter.matches(top, kinds)
                        {
                            out.push(Scoped::tagged(snap.name.as_str(), top));
                        }
                    }
                }
            }
        }
    }
    out
}

/// The `--json` payload for an analytics emit —
/// `{view, spec, format, …}` where `…` is `lanes`, `stats`, or
/// `{window, series}` per format, plus `warnings`.
pub(crate) fn emit_json(
    name: Option<&str>,
    spec: &ViewSpec,
    items: &[Scoped],
    cycles: &[CycleConfig],
    warnings: &[String],
) -> serde_json::Value {
    let mut payload = json!({
        "view": name,
        "spec": spec,
        "warnings": warnings,
    });
    match spec.format {
        ViewFormat::Timeline => {
            payload["format"] = json!("timeline");
            payload["lanes"] = json!(analytics::timeline(items.to_vec())
                .iter()
                .map(|l| json!({
                    "kind": l.kind.name(),
                    "name": l.name,
                    "items": l.items,
                }))
                .collect::<Vec<_>>());
        }
        ViewFormat::Stats => {
            payload["format"] = json!("stats");
            payload["stats"] = stats_json(&analytics::stats(items));
        }
        ViewFormat::Burnup => {
            let today = myco_ops::items::today();
            let window = analytics::burnup_window(cycles, today);
            payload["format"] = json!("burnup");
            payload["window"] = json!({
                "start": date::render(window.0),
                "end": date::render(window.1),
            });
            payload["series"] = json!(analytics::burnup(items, cycles, window)
                .iter()
                .map(|s| json!({
                    "cycle": s.cycle,
                    "points": s.points.iter().map(|p| json!({
                        "date": date::render(p.date),
                        "done": p.done,
                        "scope": p.scope,
                    })).collect::<Vec<_>>(),
                }))
                .collect::<Vec<_>>());
        }
        _ => unreachable!("emit_json only serves analytics formats"),
    }
    payload
}

/// `Stats` → `{status, priority, owner, epic, milestone, cycle,
/// label}` — each `[{name, count}]`.
fn stats_json(s: &Stats) -> serde_json::Value {
    let tallies = |v: &[analytics::Tally]| {
        json!(v
            .iter()
            .map(|t| json!({"name": t.name, "count": t.count}))
            .collect::<Vec<_>>())
    };
    json!({
        "status": tallies(&s.status),
        "priority": tallies(&s.priority),
        "owner": tallies(&s.owner),
        "epic": tallies(&s.epic),
        "milestone": tallies(&s.milestone),
        "cycle": tallies(&s.cycle),
        "label": tallies(&s.label),
    })
}

/// The tty render — deterministic ASCII; `--json` callers never
/// reach here. Resolution `warnings` already printed to stderr
/// by the caller (`view::warn`).
pub(crate) fn render(spec: &ViewSpec, items: &[Scoped], cycles: &[CycleConfig]) -> String {
    match spec.format {
        ViewFormat::Timeline => render_timeline(&analytics::timeline(items.to_vec())),
        ViewFormat::Stats => render_stats(&analytics::stats(items)),
        ViewFormat::Burnup => {
            let window = analytics::burnup_window(cycles, myco_ops::items::today());
            render_burnup(&analytics::burnup(items, cycles, window))
        }
        _ => String::new(),
    }
}

/// `kind:name (n)` lane headers + compact item lines — the same
/// `items::compact` rendering `list` uses, so a roadmap reads
/// like the ledger it projects.
fn render_timeline(lanes: &[Lane]) -> String {
    let mut out = String::new();
    for l in lanes {
        if l.items.is_empty() {
            continue;
        }
        out.push_str(&format!(
            "{}:{} ({})\n",
            l.kind.name(),
            l.name,
            l.items.len()
        ));
        for s in &l.items {
            out.push_str("  ");
            if let Some(r) = s.repo {
                out.push_str(r);
                out.push(':');
            }
            out.push_str(&crate::items::compact(s.item));
            out.push('\n');
        }
    }
    out.trim_end_matches('\n').to_string()
}

/// ASCII bars — `name` left, count, then `#` scaled so the
/// largest bucket fills 40 columns. Dimensions print only when
/// non-empty; ungrouped trails inside each.
fn render_stats(s: &Stats) -> String {
    fn dim(out: &mut String, title: &str, tallies: &[analytics::Tally]) {
        if tallies.is_empty() {
            return;
        }
        out.push_str(&format!("{title}:\n"));
        let max = tallies.iter().map(|t| t.count).max().unwrap_or(1).max(1);
        let width = tallies.iter().map(|t| t.name.len()).max().unwrap_or(0);
        for t in tallies {
            let bar = "#".repeat(t.count * 40 / max);
            out.push_str(&format!(
                "  {:<width$} {:>4}  {}\n",
                t.name,
                t.count,
                bar,
                width = width
            ));
        }
    }
    let mut out = String::new();
    dim(&mut out, "status", &s.status);
    dim(&mut out, "priority", &s.priority);
    dim(&mut out, "owner", &s.owner);
    dim(&mut out, "epic", &s.epic);
    dim(&mut out, "milestone", &s.milestone);
    dim(&mut out, "cycle", &s.cycle);
    dim(&mut out, "label", &s.label);
    if !out.is_empty() {
        out.push('\n');
    }
    out.trim_end_matches('\n').to_string()
}

/// Per-series: a header (`cycle <name> — <start> → <end>`), a
/// daily sparkline over an ASCII ramp (position = done/scope
/// fraction), and the closing `done/scope`. Unstamped archive
/// counts toward scope but never lifts the line — the curve
/// only moves on `done:` stamps.
fn render_burnup(series: &[BurnupSeries]) -> String {
    const RAMP: &[u8; 10] = b" .:-=+*#%@";
    let mut out = String::new();
    for s in series {
        let label = s.cycle.unwrap_or("uncycled");
        let (first, last) = match (s.points.first(), s.points.last()) {
            (Some(f), Some(l)) => (f.date, l.date),
            _ => continue,
        };
        let final_pt = s
            .points
            .last()
            .copied()
            .unwrap_or(myco_core::analytics::BurnupPoint {
                date: last,
                done: 0,
                scope: 0,
            });
        out.push_str(&format!(
            "{label} — {} → {}: {}/{} done\n",
            date::render(first),
            date::render(last),
            final_pt.done,
            final_pt.scope
        ));
        let line: String = s
            .points
            .iter()
            .map(|p| {
                let idx = p
                    .done
                    .checked_mul(9)
                    .and_then(|n| n.checked_div(p.scope))
                    .unwrap_or(0) as usize;
                RAMP[idx.min(9)] as char
            })
            .collect();
        out.push_str(&format!("  {line}\n"));
    }
    out.trim_end_matches('\n').to_string()
}
