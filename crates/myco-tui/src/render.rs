//! Drawing — tab bar on top, board list filling the frame, status
//! bar at the bottom. Preview mode swaps the list for the item
//! detail pane (pm's `Enter` full-pane preview).

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, ListState, Paragraph};
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

fn draw_status(f: &mut Frame, m: &Model, area: Rect) {
    let hints = match m.mode {
        Mode::Add => "enter add · esc cancel",
        Mode::Preview => "esc back",
        Mode::Normal => "j/k move · 1/2 boards · space done · D defer · p promote · alt+↑/↓ reorder · a add · enter preview · E edit · R reload · q quit",
    };
    let line = if m.mode == Mode::Add {
        Line::from(vec![
            Span::styled("add> ", Style::default().fg(ACCENT)),
            Span::raw(m.input.clone()),
            Span::styled("█", Style::default().fg(ACCENT)),
        ])
    } else if m.status.is_empty() {
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
