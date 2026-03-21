use crate::app::{App, Focus};
use crate::ui::{focused_block, C_ACCENT, C_DIM,
                C_GREEN, C_PANEL, C_RED, C_SCORE, C_TEXT};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
    Frame,
};

// ── Meta (title, score, genres, actions) ─────────────────────────────────────

pub fn draw_meta(f: &mut Frame, app: &App, area: Rect) {
    let focused = app.focus == Focus::Detail;

    let Some(item) = &app.selected else {
        f.render_widget(
            Paragraph::new(vec![
                Line::from(""),
                Line::from(Span::styled("  ← select a result", Style::default().fg(C_DIM))),
            ])
            .block(focused_block(" INFO ", focused)),
            area,
        );
        return;
    };

    let mut lines: Vec<Line> = vec![Line::from("")];

    // Title — bold white, all caps feel
    lines.push(Line::from(Span::styled(
        item.title(),
        Style::default().fg(C_TEXT).add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));

    // Score
    if let Some(s) = item.score() {
        let color = if s >= 8.5 { C_GREEN } else if s >= 7.0 { C_SCORE } else { C_DIM };
        lines.push(Line::from(vec![
            Span::styled("  ★ ", Style::default().fg(color)),
            Span::styled(
                format!("{s:.1}  {}", stars(s)),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
        ]));
    }

    // Year · episodes · status
    {
        let mut row = vec![Span::styled("  ", Style::default())];
        if let Some(y) = item.year() {
            row.push(Span::styled(y.to_string(), Style::default().fg(C_DIM)));
        }
        if let Some(ep) = item.episodes_or_chapters() {
            row.push(Span::styled("  ·  ", Style::default().fg(Color::Rgb(40,40,40))));
            row.push(Span::styled(ep, Style::default().fg(C_DIM)));
        }
        lines.push(Line::from(row));
    }

    if let Some(status) = item.status() {
        let (dot, col) = status_dot(status);
        lines.push(Line::from(vec![
            Span::styled(format!("  {dot} "), Style::default().fg(col)),
            Span::styled(status, Style::default().fg(col)),
        ]));
    }

    lines.push(Line::from(""));

    // Genres — white on dark-grey chips
    let genres = item.genres();
    if !genres.is_empty() {
        let mut row = vec![Span::styled(" ", Style::default())];
        for g in genres.iter().take(4) {
            row.push(Span::styled(
                format!(" {g} "),
                Style::default()
                    .fg(C_TEXT)
                    .bg(Color::Rgb(30, 30, 30))
                    .add_modifier(Modifier::BOLD),
            ));
            row.push(Span::styled(" ", Style::default()));
        }
        lines.push(Line::from(row));
        lines.push(Line::from(""));
    }

    // Source badge
    lines.push(Line::from(vec![
        Span::styled("  ", Style::default()),
        Span::styled(
            format!(" {} ", item.source_badge()),
            Style::default().fg(C_DIM).bg(Color::Rgb(22,22,22)),
        ),
    ]));

    lines.push(Line::from(""));

    // Action hints — yellow accent keys
    lines.push(Line::from(vec![
        Span::styled("  ", Style::default()),
        key_chip("[P]"), Span::styled(" Play  ", Style::default().fg(C_DIM)),
    ]));

    f.render_widget(
        Paragraph::new(lines)
            .block(focused_block(" INFO ", focused))
            .wrap(Wrap { trim: true }),
        area,
    );
}

// ── Synopsis ──────────────────────────────────────────────────────────────────

pub fn draw_synopsis(f: &mut Frame, app: &App, area: Rect) {
    let focused = app.focus == Focus::Detail;
    let text = app.selected.as_ref()
        .map(|i| i.synopsis())
        .unwrap_or("No synopsis.");

    f.render_widget(
        Paragraph::new(text)
            .style(Style::default().fg(Color::Rgb(180, 180, 180)))
            .block(focused_block(" SYNOPSIS ", focused))
            .wrap(Wrap { trim: true })
            .scroll((app.detail_scroll, 0)),
        area,
    );
}

// ── Episode Grid (Inline) ──────────────────────────────────────────────────────

pub fn draw_episode_grid(f: &mut Frame, app: &mut App, area: Rect) {
    use crate::app::Focus;
    let focused = app.focus == Focus::EpisodePrompt;

    let title = format!(" EPISODES ({}) ", app.stream_mode.to_uppercase());
    let block = focused_block(&title, focused);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let rows = Layout::vertical([
        Constraint::Length(2),   // input row
        Constraint::Min(0),      // grid
        Constraint::Length(1),   // hints
    ]).split(inner);

    // Input row
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" Episode: ", Style::default().fg(C_DIM)),
            Span::styled(&app.episode_input, Style::default().fg(C_TEXT).add_modifier(Modifier::BOLD)),
            Span::styled(if focused { "▌" } else { "" }, Style::default().fg(C_ACCENT)),
            Span::styled(
                format!("   {} available", app.episode_list.len()),
                Style::default().fg(C_DIM),
            ),
            Span::styled(
                format!("  ·  Quality: {}", app.stream_quality),
                Style::default().fg(Color::Rgb(60,60,60)),
            ),
        ]))
        .style(Style::default().bg(C_PANEL)),
        rows[0],
    );

    // Episode grid
    let cols_per_row = ((rows[1].width as usize).saturating_sub(2)) / 8;
    let cols_per_row = cols_per_row.max(4);
    // Sync to app so the key handler uses the exact same column count as the rendered grid
    app.episode_cols = cols_per_row;

    let current_ep = app.episode_input.trim().to_string();

    let mut grid_lines: Vec<Line> = Vec::new();
    let eps = &app.episode_list;

    if eps.is_empty() {
        grid_lines.push(Line::from(""));
        grid_lines.push(Line::from(Span::styled("  Loading episodes…", Style::default().fg(C_DIM))));
    } else {
        for chunk in eps.chunks(cols_per_row) {
            let mut spans: Vec<Span> = vec![Span::raw(" ")];
            for ep in chunk {
                let is_selected = ep == &current_ep;
                let rec = app.anime_episode_records.get(ep.as_str());

                // Compute progress fraction (0.0–1.0)
                let pct = rec.and_then(|r| {
                    if r.fully_watched { Some(1.0f64) }
                    else if r.duration_seconds > 0.0 && r.position_seconds > 0.0 {
                        Some((r.position_seconds / r.duration_seconds).clamp(0.0, 1.0))
                    } else if r.position_seconds > 0.0 {
                        Some((r.position_seconds / 1440.0).clamp(0.0, 0.95))
                    } else { None }
                });

                // Cell is " 123 " = 5 chars wide
                let cell_w = 5usize;
                let ep_display = format!(" {:>3} ", ep);

                // Fill color: ≥90% → teal (watched), <90% → yellow (in-progress)
                // Each has a normal shade and a brighter shade for when selected
                let (fill_bg_normal, fill_bg_bright) = if pct.map(|p| p >= 0.90).unwrap_or(false) {
                    (Color::Rgb(0, 160, 120), Color::Rgb(0, 200, 155))   // teal
                } else {
                    (Color::Rgb(160, 130, 0), Color::Rgb(200, 165, 0))   // yellow
                };

                let empty_bg = Color::Rgb(28, 28, 28);
                let empty_fg = Color::Rgb(90, 90, 90);
                let fill_fg  = Color::Rgb(0, 0, 0);

                if is_selected && focused {
                    if let Some(p) = pct {
                        // Cursor on a progress cell:
                        // Filled part: bright fill + WHITE text (not black) so cursor pops
                        // Unfilled part: bright white bg to clearly show cursor boundary
                        let filled_chars = (p * cell_w as f64).round() as usize;
                        let filled_chars = filled_chars.min(cell_w);
                        let chars: Vec<char> = ep_display.chars().collect();
                        let filled_str: String = chars[..filled_chars].iter().collect();
                        let empty_str:  String = chars[filled_chars..].iter().collect();
                        if !filled_str.is_empty() {
                            spans.push(Span::styled(
                                filled_str,
                                // White text on bright fill — high contrast vs dim non-selected cells
                                Style::default().fg(Color::Rgb(255,255,255)).bg(fill_bg_bright).add_modifier(Modifier::BOLD),
                            ));
                        }
                        if !empty_str.is_empty() {
                            // Bright white background for unfilled portion — cursor is always visible
                            spans.push(Span::styled(
                                empty_str,
                                Style::default().fg(Color::Rgb(0,0,0)).bg(Color::Rgb(220,220,220)).add_modifier(Modifier::BOLD),
                            ));
                        }
                    } else {
                        // Selected, no progress — solid accent
                        spans.push(Span::styled(
                            ep_display,
                            Style::default().fg(Color::Rgb(0,0,0)).bg(C_ACCENT).add_modifier(Modifier::BOLD),
                        ));
                    }
                } else if is_selected {
                    // Selected but not focused
                    spans.push(Span::styled(
                        ep_display,
                        Style::default().fg(C_TEXT).bg(Color::Rgb(60,60,60)).add_modifier(Modifier::BOLD),
                    ));
                } else if let Some(p) = pct {
                    // Has progress — split cell into filled and unfilled
                    let filled_chars = (p * cell_w as f64).round() as usize;
                    let filled_chars = filled_chars.min(cell_w);

                    let chars: Vec<char> = ep_display.chars().collect();
                    let filled_str: String = chars[..filled_chars].iter().collect();
                    let empty_str:  String = chars[filled_chars..].iter().collect();

                    if !filled_str.is_empty() {
                        spans.push(Span::styled(
                            filled_str,
                            Style::default().fg(fill_fg).bg(fill_bg_normal),
                        ));
                    }
                    if !empty_str.is_empty() {
                        spans.push(Span::styled(
                            empty_str,
                            Style::default().fg(empty_fg).bg(empty_bg),
                        ));
                    }
                } else {
                    // Unwatched
                    spans.push(Span::styled(
                        ep_display,
                        Style::default().fg(Color::Rgb(180,180,180)).bg(Color::Rgb(28,28,28)),
                    ));
                }
                spans.push(Span::styled(" ", Style::default().bg(C_PANEL)));
            }
            grid_lines.push(Line::from(spans));
        }
    }

    f.render_widget(
        Paragraph::new(grid_lines).style(Style::default().bg(C_PANEL)),
        rows[1],
    );

    // Hints
    if focused {
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" [↵] PLAY", Style::default().fg(C_ACCENT).add_modifier(Modifier::BOLD)),
                Span::styled("   [hl/←→] col   [jk/↑↓] row   [TAB] sub/dub   [Ctrl+Q] quality   [ESC] detail", Style::default().fg(Color::Rgb(50,50,50))),
            ])).style(Style::default().bg(C_PANEL)),
            rows[2],
        );
    } else {
        // When not focused, just show prompt to press P
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" Press ", Style::default().fg(C_DIM)),
                crate::ui::detail::key_chip("[P]"),
                Span::styled(" to select episode", Style::default().fg(C_DIM)),
            ])).style(Style::default().bg(C_PANEL)),
            rows[2],
        );
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn stars(s: f32) -> &'static str {
    match s as u32 { 9..=10=>"★★★★★", 8=>"★★★★☆", 7=>"★★★☆☆", 6=>"★★☆☆☆", _=>"★☆☆☆☆" }
}

fn status_dot(s: &str) -> (&'static str, Color) {
    let l = s.to_lowercase();
    if l.contains("finish") || l.contains("complet") || l.contains("released") { ("●", C_GREEN) }
    else if l.contains("airing") || l.contains("ongoing") || l.contains("returning") { ("●", C_ACCENT) }
    else if l.contains("cancel") { ("●", C_RED) }
    else { ("○", C_DIM) }
}

fn key_chip(s: &'static str) -> Span<'static> {
    Span::styled(s, Style::default()
        .fg(Color::Rgb(0,0,0))
        .bg(C_ACCENT)
        .add_modifier(Modifier::BOLD))
}