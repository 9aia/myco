//! Analytics emits — roadmap/timeline, stats, and burnup over the
//! filtered view universe (P-012). Pure domain: callers hand in
//! filter-applied [`Scoped`] items (the same universe `eval`
//! consumes); [`myco-cli`] renders the results. The design
//! principle holds — **fields are data, scheduling is config,
//! views are emits**: these functions only read `Item` fields and
//! `[[cycles]]` config, never ledger state.
//!
//! - [`timeline`] — epic → milestone → cycle lanes, items placed
//!   by `due:`. An item lands in exactly one lane: its most
//!   specific scheduling field wins (`cycle` > `milestone` >
//!   `epic`); items with none land in `ungrouped`.
//! - [`stats`] — count-by-dimension tallies (status, priority,
//!   owner, epic, milestone, cycle, label).
//! - [`burnup`] — cumulative `done:`-stamped completions vs total
//!   scope per cycle (or one series when nothing is cycled).
//!   Items archived before the stamp existed count toward scope
//!   but never the curve — their completion date is unknowable.

use std::collections::BTreeMap;

use time::{Date, Duration};

use crate::view::{Scoped, UNGROUPED};
use crate::{CycleConfig, Item, Priority, Status};

/// Which scheduling dimension a timeline lane groups by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaneKind {
    /// `epic:<name>` — the broadest lane.
    Epic,
    /// `ms:<name>` — narrower than epic.
    Milestone,
    /// `cycle:<name>` — the iteration window.
    Cycle,
    /// No scheduling field — always trails.
    Ungrouped,
}

impl LaneKind {
    /// The lane-kind token — `epic|milestone|cycle|ungrouped`.
    pub fn name(self) -> &'static str {
        match self {
            LaneKind::Epic => "epic",
            LaneKind::Milestone => "milestone",
            LaneKind::Cycle => "cycle",
            LaneKind::Ungrouped => "ungrouped",
        }
    }

    /// Sort rank — epic lanes first, ungrouped last.
    fn rank(self) -> usize {
        match self {
            LaneKind::Epic => 0,
            LaneKind::Milestone => 1,
            LaneKind::Cycle => 2,
            LaneKind::Ungrouped => 3,
        }
    }
}

/// One roadmap lane — every item carrying the lane's scheduling
/// value (as its most specific field) lands here, `due:`-sorted.
#[derive(Debug, PartialEq, Eq)]
pub struct Lane<'a> {
    /// The dimension this lane groups by.
    pub kind: LaneKind,
    /// The epic/milestone/cycle name, or [`UNGROUPED`].
    pub name: String,
    /// Lane items — `due:` ascending, undated last, id tiebreak.
    pub items: Vec<Scoped<'a>>,
}

/// The lane an item belongs to — most specific scheduling field
/// wins: `cycle` > `milestone` > `epic` > ungrouped.
fn lane_of(item: &Item) -> (LaneKind, &str) {
    if let Some(c) = &item.cycle {
        (LaneKind::Cycle, c.as_str())
    } else if let Some(m) = &item.milestone {
        (LaneKind::Milestone, m.as_str())
    } else if let Some(e) = &item.epic {
        (LaneKind::Epic, e.as_str())
    } else {
        (LaneKind::Ungrouped, UNGROUPED)
    }
}

fn due_then_id(a: &Scoped, b: &Scoped) -> std::cmp::Ordering {
    a.item
        .due
        .cmp(&b.item.due)
        .then_with(|| a.item.id.cmp(&b.item.id))
}

/// Roadmap lanes over the (already filtered) universe. Lane
/// order: epic lanes alphabetical, then milestone, then cycle,
/// `ungrouped` last — the epic → milestone → cycle hierarchy the
/// proposal fixes.
pub fn timeline<'a>(mut items: Vec<Scoped<'a>>) -> Vec<Lane<'a>> {
    let mut lanes: BTreeMap<(usize, String), Vec<Scoped>> = BTreeMap::new();
    for s in items.drain(..) {
        let (kind, name) = lane_of(s.item);
        lanes
            .entry((kind.rank(), name.to_string()))
            .or_default()
            .push(s);
    }
    lanes
        .into_iter()
        .map(|((rank, name), mut items)| {
            items.sort_by(due_then_id);
            Lane {
                kind: match rank {
                    0 => LaneKind::Epic,
                    1 => LaneKind::Milestone,
                    2 => LaneKind::Cycle,
                    _ => LaneKind::Ungrouped,
                },
                name,
                items,
            }
        })
        .collect()
}

/// One count-by-name bucket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tally {
    /// Bucket name — a status/priority token, owner, group name,
    /// label, or [`UNGROUPED`].
    pub name: String,
    /// Items in the bucket.
    pub count: usize,
}

/// The stats emit — every dimension counted over the same
/// universe; empty dimensions yield empty vecs (renderers skip
/// them).
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Stats {
    /// By lifecycle status — canonical [`Status::ALL`] order.
    pub status: Vec<Tally>,
    /// By priority — crit-first.
    pub priority: Vec<Tally>,
    /// By owner — alphabetical, unclaimed last.
    pub owner: Vec<Tally>,
    /// By epic — alphabetical, ungrouped last.
    pub epic: Vec<Tally>,
    /// By milestone — alphabetical, ungrouped last.
    pub milestone: Vec<Tally>,
    /// By cycle — alphabetical, ungrouped last.
    pub cycle: Vec<Tally>,
    /// By label — every observed `#label`, alphabetical.
    pub label: Vec<Tally>,
}

fn tally_named<'a>(
    items: impl Iterator<Item = &'a Item>,
    pick: impl Fn(&'a Item) -> Option<&'a str>,
) -> Vec<Tally> {
    let mut named: BTreeMap<&str, usize> = BTreeMap::new();
    let mut ungrouped = 0usize;
    for i in items {
        match pick(i) {
            Some(v) => *named.entry(v).or_default() += 1,
            None => ungrouped += 1,
        }
    }
    let mut out: Vec<Tally> = named
        .into_iter()
        .map(|(name, count)| Tally {
            name: name.to_string(),
            count,
        })
        .collect();
    if ungrouped > 0 {
        out.push(Tally {
            name: UNGROUPED.to_string(),
            count: ungrouped,
        });
    }
    out
}

/// Count-by-dimension tallies over the filtered universe.
/// Children count too when the caller fed them — stats are
/// counts, not rows.
pub fn stats<'a>(items: &[Scoped<'a>]) -> Stats {
    let mut status_counts = vec![0usize; Status::ALL.len()];
    let mut pri_counts = vec![0usize; Priority::ALL.len()];
    let mut labels: BTreeMap<&str, usize> = BTreeMap::new();
    for s in items {
        status_counts[s.item.status as usize] += 1;
        pri_counts[s.item.priority as usize] += 1;
        for l in &s.item.labels {
            *labels.entry(l.as_str()).or_default() += 1;
        }
    }
    Stats {
        status: Status::ALL
            .iter()
            .enumerate()
            .filter(|(i, _)| status_counts[*i] > 0)
            .map(|(_, s)| Tally {
                name: s.token().to_string(),
                count: status_counts[*s as usize],
            })
            .collect(),
        priority: Priority::ALL
            .iter()
            .rev()
            .filter(|p| pri_counts[**p as usize] > 0)
            .map(|p| Tally {
                name: p.name().to_string(),
                count: pri_counts[*p as usize],
            })
            .collect(),
        owner: tally_named(items.iter().map(|s| s.item), |i| {
            i.owner.as_ref().map(|o| o.as_str())
        }),
        epic: tally_named(items.iter().map(|s| s.item), |i| i.epic.as_deref()),
        milestone: tally_named(items.iter().map(|s| s.item), |i| i.milestone.as_deref()),
        cycle: tally_named(items.iter().map(|s| s.item), |i| i.cycle.as_deref()),
        label: labels
            .into_iter()
            .map(|(name, count)| Tally {
                name: name.to_string(),
                count,
            })
            .collect(),
    }
}

/// One day on a burnup curve.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BurnupPoint {
    /// The day.
    pub date: Date,
    /// Items whose `done:` stamp is on or before `date`.
    pub done: u64,
    /// The series' total scope — constant across the window.
    pub scope: u64,
}

/// One burnup series — a cycle's window, or the whole ledger
/// (`cycle: None`) when items aren't cycled.
#[derive(Debug, PartialEq, Eq)]
pub struct BurnupSeries<'a> {
    /// The `[[cycles]]` name this series tracks; `None` for the
    /// uncycled remainder (always last when present).
    pub cycle: Option<&'a str>,
    /// One point per day in the window, inclusive.
    pub points: Vec<BurnupPoint>,
}

/// The burnup window — the `[[cycles]]` window covering `today`
/// when one exists (the in-flight iteration), else the trailing
/// 8 weeks. Rollover is manual: outside every declared window the
/// emit falls back rather than guessing.
pub fn burnup_window(cycles: &[CycleConfig], today: Date) -> (Date, Date) {
    if let Some(c) = cycles.iter().find(|c| c.start <= today && today <= c.end) {
        (c.start, c.end)
    } else {
        (today - Duration::days(56), today)
    }
}

/// Burnup over `[start, end]` inclusive — one series per observed
/// `cycle:` value plus one per declared-but-empty cycle, and a
/// `None` series for uncycled items (last). Scope counts every
/// item in the partition; `done` counts `done:`-stamped items
/// whose stamp falls on or before each day — unstamped archive
/// predates the field and never moves the curve.
pub fn burnup<'a>(
    items: &[Scoped<'a>],
    cycles: &'a [CycleConfig],
    window: (Date, Date),
) -> Vec<BurnupSeries<'a>> {
    let (start, end) = window;
    if end < start {
        return Vec::new();
    }
    // partition: declared cycles first (sorted), then observed
    // undeclared names, then the uncycled remainder
    let mut by_cycle: BTreeMap<Option<&str>, Vec<&Item>> = BTreeMap::new();
    for s in items {
        by_cycle
            .entry(s.item.cycle.as_deref())
            .or_default()
            .push(s.item);
    }
    let mut series_cycles: Vec<Option<&str>> = Vec::new();
    for c in cycles {
        let name = c.name.as_str();
        if !series_cycles.contains(&Some(name)) {
            series_cycles.push(Some(name));
        }
    }
    for key in by_cycle.keys() {
        if key.is_some() && !series_cycles.contains(key) {
            series_cycles.push(*key);
        }
    }
    if by_cycle.contains_key(&None) {
        series_cycles.push(None);
    }

    let days = (end - start).whole_days() as usize + 1;
    series_cycles
        .into_iter()
        .map(|cycle| {
            let members: &[&Item] = by_cycle.get(&cycle).map(Vec::as_slice).unwrap_or(&[]);
            let scope = members.len() as u64;
            let stamps: Vec<Date> = members.iter().filter_map(|i| i.done).collect();
            let points = (0..days)
                .map(|d| {
                    let date = start + Duration::days(d as i64);
                    BurnupPoint {
                        date,
                        done: stamps.iter().filter(|s| **s <= date).count() as u64,
                        scope,
                    }
                })
                .collect();
            BurnupSeries { cycle, points }
        })
        .collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::{date, ItemId, ItemKind};

    fn t(n: u32) -> Item {
        Item::new(ItemId::new(ItemKind::Task, n), "x")
    }

    fn d(s: &str) -> Date {
        date::parse(s).unwrap()
    }

    fn scoped<'a>(items: &'a [Item]) -> Vec<Scoped<'a>> {
        items.iter().map(Scoped::new).collect()
    }

    #[test]
    fn timeline_lanes_order_and_specificity() {
        let items = vec![
            t(1).epic("e1").due(d("2026-01-05")),
            t(2).epic("e1").milestone("m1").due(d("2026-01-02")),
            t(3).cycle("s1")
                .milestone("m1")
                .epic("e1")
                .due(d("2026-01-09")),
            t(4),
            t(5).epic("e2"),
        ];
        let lanes = timeline(scoped(&items));
        let names: Vec<(&str, &str)> = lanes
            .iter()
            .map(|l| (l.kind.name(), l.name.as_str()))
            .collect();
        // epic lanes first (e1 has only t1 — t2/t3 promoted),
        // then milestone m1 (t2), then cycle s1 (t3), ungrouped last
        assert_eq!(
            names,
            [
                ("epic", "e1"),
                ("epic", "e2"),
                ("milestone", "m1"),
                ("cycle", "s1"),
                ("ungrouped", UNGROUPED),
            ]
        );
        // within e1: t1 only; due ordering inside lanes
        assert_eq!(lanes[0].items[0].item.id.to_string(), "T-001");
        assert_eq!(lanes[4].items[0].item.id.to_string(), "T-004");
    }

    #[test]
    fn stats_counts_every_dimension() {
        let items = vec![
            t(1).status(Status::Done)
                .priority(Priority::Critical)
                .owner("devin".parse().unwrap())
                .epic("e1")
                .cycle("s1")
                .label("bug"),
            t(2).priority(Priority::Low).label("bug").label("ui"),
            t(3),
        ];
        let s = stats(&scoped(&items));
        // canonical status order — open before done
        assert_eq!(s.status[0].name, "open");
        assert_eq!(s.status[0].count, 2);
        assert_eq!(s.status[1].name, "done");
        assert_eq!(s.status[1].count, 1);
        assert_eq!(s.priority[0].name, "critical"); // crit-first
        assert_eq!(s.priority[1].name, "medium"); // rank order
        assert_eq!(s.priority[2].name, "low");
        assert_eq!(s.owner.len(), 2); // devin + ungrouped
        assert_eq!(s.epic[0].name, "e1");
        assert_eq!(s.cycle[0].name, "s1");
        assert_eq!(s.label.len(), 2); // bug×2, ui×1
        assert_eq!(s.label[0].name, "bug");
        assert_eq!(s.label[0].count, 2);
    }

    #[test]
    fn burnup_counts_stamps_and_scope() {
        let mut done1 = t(1);
        done1.status = Status::Done;
        done1.done = Some(d("2026-01-03"));
        let mut done2 = t(2);
        done2.status = Status::Done;
        done2.done = Some(d("2026-01-05"));
        let mut old = t(3); // archived pre-stamp: scope, never curve
        old.status = Status::Done;
        let live = t(4);
        let items = vec![done1, done2, old, live];
        let cycles = vec![CycleConfig {
            name: "s1".into(),
            start: d("2026-01-01"),
            end: d("2026-01-07"),
        }];
        let series = burnup(&scoped(&items), &cycles, (d("2026-01-01"), d("2026-01-07")));
        // one declared-cycle series (empty: no items carry cycle s1)
        // plus the uncycled remainder
        assert_eq!(series.len(), 2);
        let uncycled = &series[1];
        assert_eq!(uncycled.cycle, None);
        assert_eq!(uncycled.points.len(), 7);
        assert_eq!(uncycled.points[0].scope, 4);
        assert_eq!(uncycled.points[0].done, 0);
        assert_eq!(uncycled.points[2].done, 1); // 01-03 stamp
        assert_eq!(uncycled.points[4].done, 2); // 01-05 stamp
        assert_eq!(uncycled.points[6].done, 2); // old never counts
    }

    #[test]
    fn burnup_window_prefers_covering_cycle() {
        let cycles = vec![CycleConfig {
            name: "s1".into(),
            start: d("2026-01-01"),
            end: d("2026-01-14"),
        }];
        assert_eq!(
            burnup_window(&cycles, d("2026-01-10")),
            (d("2026-01-01"), d("2026-01-14"))
        );
        // outside every window → trailing 8 weeks
        let (s, e) = burnup_window(&cycles, d("2026-03-01"));
        assert_eq!(e, d("2026-03-01"));
        assert_eq!((e - s).whole_days(), 56);
    }
}
