pub mod detail;
pub mod history;
pub mod image;
pub mod search;

use crate::app::{App, Tab, ToastKind};
use strum::IntoEnumIterator;
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Tabs},
    Frame,
};

// ── BOLD BLACK PALETTE ────────────────────────────────────────────────────────
// Pure black base. Single neon accent. High contrast everything.

pub const C_BG:       Color = Color::Rgb(0,   0,   0  );  // pure black
pub const C_BG2:      Color = Color::Rgb(10,  10,  10 );  // slightly off-black
pub const C_BG3:      Color = Color::Rgb(18,  18,  18 );  // card bg
pub const C_PANEL:    Color = Color::Rgb(8,   8,   8  );  // panel bg
pub const C_BORDER:   Color = Color::Rgb(38,  38,  38 );  // dim border
pub const C_BORDER_F: Color = Color::Rgb(220, 220, 220);  // focused border — bright white
pub const C_TEXT:     Color = Color::Rgb(240, 240, 240);  // near-white text
pub const C_DIM:      Color = Color::Rgb(90,  90,  90 );  // muted text
pub const C_ACCENT:   Color = Color::Rgb(255, 255, 0  );  // pure yellow — the ONE accent
pub const C_GREEN:    Color = Color::Rgb(0,   255, 128);  // success / ongoing
pub const C_RED:      Color = Color::Rgb(255, 50,  50 );  // error / cancelled
// pub const C_CYAN:     Color = Color::Rgb(0,   220, 255);  // movie type badge
pub const C_SCORE:    Color = Color::Rgb(255, 200, 0  );  // score stars

// ── Master draw ───────────────────────────────────────────────────────────────

pub fn draw(f: &mut Frame, app: &mut App) {
    let area = f.area();

    // Fill entire terminal with pure black first
    f.render_widget(
        Block::default().style(Style::default().bg(C_BG)),
        area,
    );

    let root = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(0),
        Constraint::Length(1),
    ]).split(area);

    draw_header(f, app, root[0]);
    draw_body(f, app, root[1]);
    draw_statusbar(f, app, root[2]);
    draw_toasts(f, app, area);
}

// ── Header ────────────────────────────────────────────────────────────────────

fn draw_header(f: &mut Frame, app: &App, area: Rect) {
    // Fill header bg
    f.render_widget(
        Block::default().style(Style::default().bg(C_BG2)),
        area,
    );

    let cols = Layout::horizontal([
        Constraint::Length(10),
        Constraint::Min(0),
        Constraint::Length(36),
    ]).split(area);

    // Logo — bold white with yellow diamond
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("◆ ", Style::default().fg(C_ACCENT)),
            Span::styled("NEXUS", Style::default()
                .fg(C_TEXT)
                .add_modifier(Modifier::BOLD)),
        ]))
        .style(Style::default().bg(C_BG2))
        .block(Block::default()
            .borders(Borders::BOTTOM)
            .border_style(Style::default().fg(C_BORDER))),
        cols[0],
    );

    // Tabs
    let labels: Vec<Line> = Tab::iter().map(|t| {
        let active = t == app.active_tab;
        let label = match t {
            Tab::Anime   => " ANIME ",
            Tab::History => " HISTORY ",
        };
        if active {
            Line::from(Span::styled(label, Style::default()
                .fg(C_BG)
                .bg(C_ACCENT)
                .add_modifier(Modifier::BOLD)))
        } else {
            Line::from(Span::styled(label, Style::default().fg(C_DIM)))
        }
    }).collect();

    f.render_widget(
        Tabs::new(labels)
            .select(tab_idx(&app.active_tab))
            .block(Block::default()
                .borders(Borders::BOTTOM)
                .border_style(Style::default().fg(C_BORDER))
                .style(Style::default().bg(C_BG2)))
            .divider(Span::styled(" ", Style::default())),
        cols[1],
    );

    if app.active_tab == Tab::Anime {
        search::draw_search_bar(f, app, cols[2]);
    }
}

// ── Body ──────────────────────────────────────────────────────────────────────

fn draw_body(f: &mut Frame, app: &mut App, area: Rect) {
    // Fill body
    f.render_widget(
        Block::default().style(Style::default().bg(C_BG)),
        area,
    );

    if app.active_tab == Tab::History {
        history::draw(f, app, area);
        return;
    }

    let cols = Layout::horizontal([
        Constraint::Percentage(26),
        Constraint::Percentage(74),
    ]).split(area);

    search::draw_results(f, app, cols[0]);
    draw_right(f, app, cols[1]);
}

fn draw_right(f: &mut Frame, app: &mut App, area: Rect) {
    let rows = Layout::vertical([
        Constraint::Percentage(55),
        Constraint::Min(0),
    ]).split(area);

    let top = Layout::horizontal([
        Constraint::Percentage(35),
        Constraint::Percentage(30),
        Constraint::Min(0),
    ]).split(rows[0]);

    image::draw_cover(f, app, top[0]);
    crate::ui::detail::draw_meta(f, app, top[1]);
    crate::ui::detail::draw_synopsis(f, app, top[2]);

    crate::ui::detail::draw_episode_grid(f, app, rows[1]);
}

// ── Status bar ────────────────────────────────────────────────────────────────

fn draw_statusbar(f: &mut Frame, app: &App, area: Rect) {
    let spin = if app.is_searching {
        format!("{} ", app.spinner.symbol())
    } else { "  ".into() };

    let more_hint = if app.has_more { "  [Ctrl+N] more" } else { "" };

    let line = Line::from(vec![
        Span::styled(spin, Style::default().fg(C_ACCENT)),
        Span::styled(&app.status, Style::default().fg(C_DIM)),
        Span::styled(more_hint, Style::default().fg(Color::Rgb(55, 55, 55))),
        Span::styled(
            "    [/] search  [jk] nav  [l→] detail  [Tab] recs  [Ctrl+↑↓] panes  [p] play  [q] quit",
            Style::default().fg(Color::Rgb(40, 40, 40)),
        ),
    ]);

    f.render_widget(
        Paragraph::new(line).style(Style::default().bg(C_BG2)),
        area,
    );
}

// ── Toast overlay ─────────────────────────────────────────────────────────────

fn draw_toasts(f: &mut Frame, app: &App, area: Rect) {
    if app.toasts.is_empty() { return; }

    let w: u16 = 52;
    let x = area.right().saturating_sub(w + 1);
    let mut y = area.bottom().saturating_sub(2);

    for toast in app.toasts.iter().rev() {
        if y < area.y + 3 { break; }
        y = y.saturating_sub(3);

        let (border_col, icon) = match toast.kind {
            ToastKind::Info    => (C_DIM,   "─"),
            ToastKind::Success => (C_GREEN, "✓"),
            ToastKind::Error   => (C_RED,   "✗"),
        };

        let rect = Rect { x, y, width: w, height: 3 };
        f.render_widget(Clear, rect);
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(format!("{icon} "), Style::default().fg(border_col)),
                Span::styled(
                    trunc(&toast.message, (w - 5) as usize),
                    Style::default().fg(C_TEXT),
                ),
            ]))
            .block(Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(border_col))
                .style(Style::default().bg(C_BG3))),
            rect,
        );
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

pub fn tab_idx(tab: &Tab) -> usize {
    match tab { Tab::Anime => 0, Tab::History => 1 }
}

pub fn focused_block<'a>(title: &'a str, focused: bool) -> Block<'a> {
    Block::default()
        .title(Span::styled(title, Style::default()
            .fg(if focused { C_ACCENT } else { C_DIM })
            .add_modifier(if focused { Modifier::BOLD } else { Modifier::empty() })))
        .borders(Borders::ALL)
        .border_style(Style::default()
            .fg(if focused { C_BORDER_F } else { C_BORDER }))
        .style(Style::default().bg(C_PANEL))
}

pub fn trunc(s: &str, max: usize) -> String {
    let c: Vec<char> = s.chars().collect();
    if c.len() <= max { s.to_string() }
    else { c[..max.saturating_sub(1)].iter().collect::<String>() + "…" }
}

// (draw_episode_prompt moved to detail::draw_episode_grid)