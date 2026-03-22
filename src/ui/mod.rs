pub mod detail;
pub mod history;
pub mod image;
pub mod search;
pub mod settings;

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

pub const C_BG:       Color = Color::Rgb(0,   0,   0  );
pub const C_BG2:      Color = Color::Rgb(10,  10,  10 );
pub const C_BG3:      Color = Color::Rgb(18,  18,  18 );
pub const C_PANEL:    Color = Color::Rgb(8,   8,   8  );
pub const C_BORDER:   Color = Color::Rgb(38,  38,  38 );
pub const C_BORDER_F: Color = Color::Rgb(220, 220, 220);
pub const C_TEXT:     Color = Color::Rgb(240, 240, 240);
pub const C_DIM:      Color = Color::Rgb(90,  90,  90 );
pub const C_ACCENT:   Color = Color::Rgb(255, 255, 0  ); // default yellow — overridden at runtime
pub const C_GREEN:    Color = Color::Rgb(0,   255, 128);
pub const C_RED:      Color = Color::Rgb(255, 50,  50 );
pub const C_SCORE:    Color = Color::Rgb(255, 200, 0  );

// ── Runtime accent — updated each frame from app.config ───────────────────────

use std::cell::Cell;

thread_local! {
    static ACCENT_R:   Cell<u8> = Cell::new(255);
    static ACCENT_G:   Cell<u8> = Cell::new(255);
    static ACCENT_B:   Cell<u8> = Cell::new(0);
    static BAR_PROG_R: Cell<u8> = Cell::new(255);
    static BAR_PROG_G: Cell<u8> = Cell::new(255);
    static BAR_PROG_B: Cell<u8> = Cell::new(0);
    static BAR_DONE_R: Cell<u8> = Cell::new(0);
    static BAR_DONE_G: Cell<u8> = Cell::new(200);
    static BAR_DONE_B: Cell<u8> = Cell::new(150);
}

pub fn set_accent(r: u8, g: u8, b: u8) {
    ACCENT_R.with(|c| c.set(r));
    ACCENT_G.with(|c| c.set(g));
    ACCENT_B.with(|c| c.set(b));
}

pub fn set_bar_colors(pr: u8, pg: u8, pb: u8, dr: u8, dg: u8, db: u8) {
    BAR_PROG_R.with(|c| c.set(pr));
    BAR_PROG_G.with(|c| c.set(pg));
    BAR_PROG_B.with(|c| c.set(pb));
    BAR_DONE_R.with(|c| c.set(dr));
    BAR_DONE_G.with(|c| c.set(dg));
    BAR_DONE_B.with(|c| c.set(db));
}

pub fn accent() -> Color {
    Color::Rgb(ACCENT_R.with(|c| c.get()), ACCENT_G.with(|c| c.get()), ACCENT_B.with(|c| c.get()))
}

pub fn accent_dim() -> Color {
    let r = ACCENT_R.with(|c| c.get()) / 4;
    let g = ACCENT_G.with(|c| c.get()) / 4;
    let b = ACCENT_B.with(|c| c.get()) / 4;
    Color::Rgb(r.max(20), g.max(20), b.max(20))
}

/// In-progress episode bar color (<90%).
pub fn bar_progress() -> Color {
    Color::Rgb(BAR_PROG_R.with(|c| c.get()), BAR_PROG_G.with(|c| c.get()), BAR_PROG_B.with(|c| c.get()))
}

/// Brighter in-progress bar — used for selected/focused cells.
pub fn bar_progress_bright() -> Color {
    let r = (BAR_PROG_R.with(|c| c.get()) as u16 + 40).min(255) as u8;
    let g = (BAR_PROG_G.with(|c| c.get()) as u16 + 40).min(255) as u8;
    let b = (BAR_PROG_B.with(|c| c.get()) as u16 + 40).min(255) as u8;
    Color::Rgb(r, g, b)
}

/// Complete episode bar color (≥90%).
pub fn bar_complete() -> Color {
    Color::Rgb(BAR_DONE_R.with(|c| c.get()), BAR_DONE_G.with(|c| c.get()), BAR_DONE_B.with(|c| c.get()))
}

/// Dimmed complete bar — used for selected cell fill background.
pub fn bar_complete_dim() -> Color {
    let r = BAR_DONE_R.with(|c| c.get()) / 2;
    let g = BAR_DONE_G.with(|c| c.get()) / 2;
    let b = BAR_DONE_B.with(|c| c.get()) / 2;
    Color::Rgb(r.max(15), g.max(15), b.max(15))
}

// ── Master draw ───────────────────────────────────────────────────────────────

pub fn draw(f: &mut Frame, app: &mut App) {
    let (r, g, b) = crate::config::Config::color_rgb(&app.config.theme.accent);
    set_accent(r, g, b);
    let (pr, pg, pb) = crate::config::Config::color_rgb(&app.config.theme.bar_progress);
    let (dr, dg, db) = crate::config::Config::color_rgb(&app.config.theme.bar_complete);
    set_bar_colors(pr, pg, pb, dr, dg, db);
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
            Span::styled("◆ ", Style::default().fg(crate::ui::accent())),
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
            Tab::Anime    => " ANIME ",
            Tab::History  => " HISTORY ",
            Tab::Settings => " SETTINGS ",
        };
        if active {
            Line::from(Span::styled(label, Style::default()
                .fg(C_BG)
                .bg(crate::ui::accent())
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
    f.render_widget(
        Block::default().style(Style::default().bg(C_BG)),
        area,
    );

    if app.active_tab == Tab::Settings {
        settings::draw(f, app, area);
        return;
    }

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
        Span::styled(spin, Style::default().fg(crate::ui::accent())),
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
    match tab { Tab::Anime => 0, Tab::History => 1, Tab::Settings => 2 }
}

pub fn focused_block<'a>(title: &'a str, focused: bool) -> Block<'a> {
    Block::default()
        .title(Span::styled(title, Style::default()
            .fg(if focused { crate::ui::accent() } else { C_DIM })
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