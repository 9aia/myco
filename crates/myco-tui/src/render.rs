//! Drawing — tab bar on top, board list filling the frame, status
//! bar at the bottom. Preview mode swaps the list for the item
//! detail pane (pm's `Enter` full-pane preview).

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph};
use ratatui::Frame;

use crate::app::{Board, Mode, Model, Row};

const ACCENT: Color = Color::Indexed(69); // pm's nav blue
const MUTED: Color = Color::Indexed(241);

/// One full frame.
pub fn draw(f: &mut Frame, m: &Model) {
    let area = f.area();
    let [tabs, body, status] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(area);

    draw_tabs(f, m, tabs);
    match m.mode {
        Mode::Preview => draw_preview(f, m, body),
        _ => draw_board(f, m, body),
    }
    draw_status(f, m, status);
    if m.mode == Mode::Help {
        draw_help(f, area);
    }
}

fn draw_tabs(f: &mut Frame, m: &Model, area: Rect) {
    let mut spans = vec![Span::raw(" ")];
    for b in Board::ALL {
        let label = format!(" {} ", b.label());
        let style = if b == m.board {
            Style::default().fg(Color::White).bg(ACCENT)
        } else {
            Style::default().fg(MUTED)
        };
        spans.push(Span::styled(label, style));
        spans.push(Span::raw(" "));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn draw_board(f: &mut Frame, m: &Model, area: Rect) {
    let items: Vec<ListItem> = m
        .rows
        .iter()
        .map(|r| match r {
            Row::Header(name) => ListItem::new(Line::from(Span::styled(
                format!("## {name}"),
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ))),
            Row::Item { item, depth } => {
                let indent = "  ".repeat(*depth);
                let mut spans = vec![
                    Span::raw(indent),
                    Span::styled(item.id.to_string(), Style::default().fg(ACCENT)),
                    Span::raw(" "),
                    Span::raw(item.title.clone()),
                ];
                let mut meta = format!(" {}", item.status);
                if !matches!(item.priority, myco_core::Priority::Medium) {
                    meta += &format!(" pri:{}", item.priority.token());
                }
                for l in &item.labels {
                    meta += &format!(" #{l}");
                }
                spans.push(Span::styled(meta, Style::default().fg(MUTED)));
                ListItem::new(Line::from(spans))
            }
        })
        .collect();

    let list = List::new(items)
        .highlight_style(Style::default().bg(Color::Indexed(238)))
        .highlight_symbol("▸ ");
    let mut state = ListState::default().with_selected(Some(m.sel));
    f.render_stateful_widget(list, area, &mut state);
}

fn draw_preview(f: &mut Frame, m: &Model, area: Rect) {
    let Some(item) = m.selected() else {
        return;
    };
    let mut lines = vec![
        Line::from(Span::styled(
            item.id.to_string(),
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            item.title.clone(),
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("status ", Style::default().fg(MUTED)),
            Span::raw(item.status.to_string()),
        ]),
        Line::from(vec![
            Span::styled("pri    ", Style::default().fg(MUTED)),
            Span::raw(item.priority.token().to_string()),
        ]),
    ];
    if let Some(o) = &item.owner {
        lines.push(Line::from(vec![
            Span::styled("owner  ", Style::default().fg(MUTED)),
            Span::raw(o.to_string()),
        ]));
    }
    if !item.labels.is_empty() {
        lines.push(Line::from(vec![
            Span::styled("labels ", Style::default().fg(MUTED)),
            Span::raw(
                item.labels
                    .iter()
                    .map(|l| format!("#{l}"))
                    .collect::<Vec<_>>()
                    .join(" "),
            ),
        ]));
    }
    for (key, val) in [("epic   ", &item.epic), ("ms     ", &item.milestone)] {
        if let Some(v) = val {
            lines.push(Line::from(vec![
                Span::styled(key, Style::default().fg(MUTED)),
                Span::raw(v.clone()),
            ]));
        }
    }
    if let Some(d) = item.created {
        lines.push(Line::from(vec![
            Span::styled("created", Style::default().fg(MUTED)),
            Span::raw(format!(" {d}")),
        ]));
    }
    if !item.children.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "children",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        )));
        for c in &item.children {
            lines.push(Line::from(format!("  {}", myco_ops::items::compact(c))));
        }
    }
    if let Some(note) = &item.note {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "note",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(note.clone()));
    }
    f.render_widget(Paragraph::new(lines), area);
}

/// Normal-mode hint sets, widest first — `pick_hints` takes the
/// first that fits so narrow terminals degrade instead of cropping.
const HINTS: [&str; 3] = [
    "j/k move · 1/2 boards · space done · D defer · p promote · alt+↑/↓ reorder · a add · enter preview · E edit · R reload · ? help · q quit",
    "j/k move · space done · a add · enter · E edit · ? help · q quit",
    "? help · q quit",
];

/// The hint line for `mode` that fits `width` columns (including the
/// leading space). The minimal set always wins as the fallback.
fn pick_hints(mode: Mode, width: usize) -> &'static str {
    match mode {
        Mode::Add => "enter add · esc cancel",
        Mode::Preview => "esc back",
        Mode::Help => "esc/?/q close",
        Mode::Normal => HINTS
            .iter()
            .copied()
            .find(|h| h.chars().count() < width)
            .unwrap_or(HINTS[HINTS.len() - 1]),
    }
}

fn draw_status(f: &mut Frame, m: &Model, area: Rect) {
    if m.mode == Mode::Add {
        let line = Line::from(vec![
            Span::styled("add> ", Style::default().fg(ACCENT)),
            Span::raw(m.input.clone()),
            Span::styled("█", Style::default().fg(ACCENT)),
        ]);
        f.render_widget(Paragraph::new(line), area);
        return;
    }
    // The status message eats columns first; hints get what remains.
    let status_w = if m.status.is_empty() {
        0
    } else {
        m.status.chars().count() + 2
    };
    let hints = pick_hints(m.mode, (area.width as usize).saturating_sub(status_w));
    let line = if m.status.is_empty() {
        Line::from(Span::styled(
            format!(" {hints}"),
            Style::default().fg(MUTED),
        ))
    } else {
        Line::from(vec![
            Span::styled(format!(" {} ", m.status), Style::default().fg(Color::White)),
            Span::styled(format!(" {hints}"), Style::default().fg(MUTED)),
        ])
    };
    f.render_widget(Paragraph::new(line), area);
}

/// A `width`×`height` rect centered in `area`, clamped to fit.
fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect {
        x: area.x + (area.width - width) / 2,
        y: area.y + (area.height - height) / 2,
        width,
        height,
    }
}

/// The `?` overlay — every bind, grouped like the status bar hints.
fn help_lines() -> Vec<Line<'static>> {
    fn group(name: &'static str) -> Line<'static> {
        Line::from(Span::styled(
            name,
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ))
    }
    fn bind(keys: &'static str, desc: &'static str) -> Line<'static> {
        Line::from(vec![
            Span::raw("  "),
            Span::styled(format!("{keys:<12}"), Style::default().fg(ACCENT)),
            Span::styled(desc, Style::default().fg(MUTED)),
        ])
    }
    // No blank separators — the whole list must fit a 24-row screen.
    vec![
        group("nav"),
        bind("j/k, ↑/↓", "move"),
        bind("gg / G", "first / last"),
        bind("enter", "preview item"),
        group("tabs"),
        bind("1 / 2", "TODO / BACKLOG"),
        bind("h/l, ←/→", "switch board"),
        group("items"),
        bind("space, dd", "mark done"),
        bind("D", "defer → BACKLOG"),
        bind("p", "promote → TODO"),
        bind("alt+j/k, ↑/↓", "reorder"),
        group("edit"),
        bind("a", "add item"),
        bind("E", "open in $EDITOR"),
        bind("R", "reload"),
        group("misc"),
        bind("?", "this help"),
        bind("q, ctrl+c", "quit"),
        Line::from(Span::styled(
            "esc / ? / q to close",
            Style::default().fg(MUTED),
        )),
    ]
}

fn draw_help(f: &mut Frame, area: Rect) {
    let lines = help_lines();
    let width = lines.iter().map(Line::width).max().unwrap_or(0) as u16 + 4;
    let height = lines.len() as u16 + 2;
    let popup = centered(area, width, height);
    f.render_widget(Clear, popup);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT))
        .title(Span::styled(" keys ", Style::default().fg(ACCENT)));
    f.render_widget(Paragraph::new(lines).block(block), popup);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hints_degrade_full_to_minimal() {
        let len = |h: &str| h.chars().count();
        assert_eq!(pick_hints(Mode::Normal, len(HINTS[0]) + 1), HINTS[0]);
        assert_eq!(pick_hints(Mode::Normal, len(HINTS[0])), HINTS[1]);
        assert_eq!(pick_hints(Mode::Normal, len(HINTS[1])), HINTS[2]);
        // tiny terminals still get the minimal set (cropped, not absent)
        assert_eq!(pick_hints(Mode::Normal, 0), HINTS[2]);
        assert_eq!(pick_hints(Mode::Preview, 0), "esc back");
    }
}
