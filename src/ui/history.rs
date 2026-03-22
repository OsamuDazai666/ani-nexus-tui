use crate::app::{App, Focus};
use crate::ui::{focused_block, trunc, C_BG, C_BG3,
                C_BORDER_F, C_DIM, C_PANEL, C_TEXT, C_BORDER};

// Teal — complements the yellow accent for watched/complete state

use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, Paragraph},
    Frame,
};

// ── Entry point ───────────────────────────────────────────────────────────────

pub fn draw(f: &mut Frame, app: &mut App, area: Rect) {
    let cols = Layout::horizontal([
        Constraint::Percentage(38),
        Constraint::Percentage(62),
    ]).split(area);

    draw_list(f, app, cols[0]);
    draw_detail(f, app, cols[1]);
}

// ── Left pane: filter bar + card list ─────────────────────────────────────────

fn draw_list(f: &mut Frame, app: &App, area: Rect) {
    let focused = app.focus == Focus::History;

    let rows = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(0),
    ]).split(area);

    draw_filter_bar(f, app, rows[0], focused);
    draw_cards(f, app, rows[1], focused);
}

fn draw_filter_bar(f: &mut Frame, app: &App, area: Rect, list_focused: bool) {
    let has_filter = !app.history_filter.is_empty();

    let content = if app.history_filter.is_empty() {
        Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(
                if list_focused { "type to filter…▌" } else { "type to filter…" },
                Style::default().fg(C_DIM),
            ),
        ])
    } else {
        Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(&app.history_filter, Style::default().fg(C_TEXT).add_modifier(Modifier::BOLD)),
            Span::styled(
                if list_focused { "▌" } else { "" },
                Style::default().fg(crate::ui::accent()),
            ),
            Span::styled(
                format!("  {} match{}", app.history_filtered.len(),
                    if app.history_filtered.len() == 1 { "" } else { "es" }),
                Style::default().fg(C_DIM),
            ),
        ])
    };

    let title = if has_filter {
        Span::styled(" FILTER [Esc clear] ", Style::default().fg(crate::ui::accent()).add_modifier(Modifier::BOLD))
    } else {
        Span::styled(" FILTER ", Style::default().fg(C_DIM))
    };

    f.render_widget(
        Paragraph::new(content)
            .block(Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(
                    if has_filter { crate::ui::accent() }
                    else if list_focused { C_BORDER_F }
                    else { C_BORDER }
                ))
                .style(Style::default().bg(C_PANEL))),
        area,
    );
}

fn draw_cards(f: &mut Frame, app: &App, area: Rect, list_focused: bool) {
    let count = if app.history_filter.is_empty() {
        app.history.len()
    } else {
        app.history_filtered.len()
    };

    let title_str = match count {
        0 => " HISTORY ".to_string(),
        n => format!(" HISTORY  {n} "),
    };

    let container = Block::default()
        .title(Span::styled(title_str, Style::default()
            .fg(if list_focused { crate::ui::accent() } else { C_DIM })
            .add_modifier(Modifier::BOLD)))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(if list_focused { C_BORDER_F } else { C_BORDER }))
        .style(Style::default().bg(C_PANEL));

    let inner = container.inner(area);
    f.render_widget(container, area);

    if count == 0 {
        let msg = if !app.history_filter.is_empty() {
            "  No matches."
        } else {
            "  Nothing watched yet."
        };
        f.render_widget(
            Paragraph::new(vec![
                Line::from(""),
                Line::from(Span::styled(msg, Style::default().fg(C_DIM))),
            ]),
            inner,
        );
        return;
    }

    // Each card: 5 rows tall (2 border + 3 content lines)
    let card_h: u16 = 5;
    let max_visible = (inner.height / card_h) as usize;
    if max_visible == 0 { return; }

    let start_idx = if app.history_idx >= max_visible {
        app.history_idx - (max_visible - 1)
    } else {
        0
    };

    for slot in 0..max_visible {
        let list_pos = start_idx + slot;
        if list_pos >= count { break; }

        let entry_idx = if app.history_filter.is_empty() {
            list_pos
        } else {
            match app.history_filtered.get(list_pos) {
                Some(&i) => i,
                None => break,
            }
        };

        let Some(entry) = app.history.get(entry_idx) else { break };
        let sel = list_pos == app.history_idx;

        let card_area = Rect {
            x: inner.x,
            y: inner.y + (slot as u16 * card_h),
            width: inner.width,
            height: card_h,
        };
        if card_area.y + card_area.height > inner.y + inner.height { break; }

        let border_style = if sel && list_focused {
            Style::default().fg(crate::ui::accent()).add_modifier(Modifier::BOLD)
        } else if sel {
            Style::default().fg(C_TEXT)
        } else {
            Style::default().fg(Color::Rgb(32, 32, 32))
        };

        let card_bg = if sel { C_BG3 } else { C_BG };

        let card_block = Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .style(Style::default().bg(card_bg));

        let card_inner = card_block.inner(card_area);
        f.render_widget(card_block, card_area);

        // Thumbnail column (7 cols) if cover_url exists and there's room
        let thumb_w: u16 = 7;
        if entry.cover_url.is_some() && card_inner.width > thumb_w + 6 {
            let card_cols = Layout::horizontal([
                Constraint::Length(thumb_w),
                Constraint::Min(0),
            ]).split(card_inner);

            draw_mini_thumb(f, entry, card_cols[0]);
            draw_card_text(f, entry, card_cols[1], sel, list_focused);
        } else {
            draw_card_text(f, entry, card_inner, sel, list_focused);
        }
    }
}

fn draw_mini_thumb(f: &mut Frame, entry: &crate::db::history::HistoryEntry, area: Rect) {
    let first = entry.title.chars().next()
        .and_then(|c| c.to_uppercase().next())
        .unwrap_or('?');

    let lines = vec![
        Line::from(Span::styled("┌─────┐", Style::default().fg(Color::Rgb(45, 45, 45)))),
        Line::from(vec![
            Span::styled("│  ", Style::default().fg(Color::Rgb(45, 45, 45))),
            Span::styled(first.to_string(), Style::default().fg(crate::ui::accent()).add_modifier(Modifier::BOLD)),
            Span::styled("  │", Style::default().fg(Color::Rgb(45, 45, 45))),
        ]),
        Line::from(Span::styled("└─────┘", Style::default().fg(Color::Rgb(45, 45, 45)))),
    ];
    f.render_widget(Paragraph::new(lines), area);
}

fn draw_card_text(
    f: &mut Frame,
    entry: &crate::db::history::HistoryEntry,
    area: Rect,
    sel: bool,
    focused: bool,
) {
    let title_style = if sel && focused {
        Style::default().fg(crate::ui::accent()).add_modifier(Modifier::BOLD)
    } else if sel {
        Style::default().fg(C_TEXT).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(C_TEXT)
    };

    let max_title = (area.width as usize).saturating_sub(1);
    let line1 = Line::from(Span::styled(trunc(&entry.title, max_title), title_style));

    let date_str = entry.last_watched.format("%b %d  %H:%M").to_string();
    let prog_str = match (entry.progress, entry.total) {
        (Some(p), Some(t)) => format!("Ep {p}/{t}"),
        (Some(p), None)    => format!("Ep {p}"),
        _                  => String::new(),
    };
    let play_str = if entry.play_count > 1 {
        format!("{}×", entry.play_count)
    } else {
        String::new()
    };

    let mut meta_spans = vec![Span::styled(date_str, Style::default().fg(C_DIM))];
    if !prog_str.is_empty() {
        meta_spans.push(Span::styled("  ", Style::default()));
        meta_spans.push(Span::styled(prog_str, Style::default().fg(crate::ui::bar_complete())));
    }
    if !play_str.is_empty() {
        meta_spans.push(Span::styled("  ", Style::default()));
        meta_spans.push(Span::styled(play_str, Style::default().fg(C_DIM)));
    }
    let line2 = Line::from(meta_spans);

    let bar_width = (area.width as usize).saturating_sub(1);
    let bar = entry.progress_bar(bar_width);
    let bar_color = if sel && focused { crate::ui::accent() } else { crate::ui::accent_dim() };
    let line3 = Line::from(Span::styled(bar, Style::default().fg(bar_color)));

    f.render_widget(Paragraph::new(vec![line1, line2, line3]), area);
}

// ── Right pane: detail + episode list ─────────────────────────────────────────

fn draw_detail(f: &mut Frame, app: &mut App, area: Rect) {
    let detail_focused   = app.focus == Focus::HistoryDetail;
    let episodes_focused = app.focus == Focus::HistoryEpisodes;

    let Some(entry) = app.history_selected().cloned() else {
        f.render_widget(
            Paragraph::new(vec![
                Line::from(""),
                Line::from(Span::styled("  Nothing watched yet.", Style::default().fg(C_DIM))),
                Line::from(""),
                Line::from(Span::styled(
                    "  Search something and press [P] to play.",
                    Style::default().fg(C_DIM),
                )),
            ])
            .block(focused_block(" DETAIL ", false)),
            area,
        );
        return;
    };

    // Stacked layout: metadata top, gauge, episode list bottom
    let has_episodes = !app.history_episode_list.is_empty();
    let rows = if has_episodes {
        Layout::vertical([
            Constraint::Length(12),  // metadata + cover
            Constraint::Length(3),   // progress gauge
            Constraint::Min(0),      // episode list
        ]).split(area)
    } else {
        Layout::vertical([
            Constraint::Min(0),
            Constraint::Length(3),
            Constraint::Length(0),
        ]).split(area)
    };

    // Top: cover image left, metadata right
    let top_cols = Layout::horizontal([
        Constraint::Length(22),
        Constraint::Min(0),
    ]).split(rows[0]);

    draw_detail_cover(f, app, top_cols[0], detail_focused || episodes_focused);
    draw_detail_meta(f, &entry, top_cols[1], detail_focused, episodes_focused);
    draw_detail_gauge(f, &entry, rows[1]);

    if has_episodes {
        draw_episode_list(f, app, &entry, rows[2], episodes_focused);
    }
}

fn draw_detail_cover(f: &mut Frame, app: &mut App, area: Rect, focused: bool) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(
            if focused { crate::ui::accent_dim() } else { C_BORDER }
        ))
        .style(Style::default().bg(Color::Rgb(0, 0, 0)));

    if let Some(ref mut protocol) = app.history_cover {
        let inner = block.inner(area);
        f.render_widget(block, area);
        let image_widget = ratatui_image::StatefulImage::default()
            .resize(ratatui_image::Resize::Fit(Some(
                image::imageops::FilterType::Lanczos3,
            )));
        f.render_stateful_widget(image_widget, inner, protocol);
    } else {
        let inner = block.inner(area);
        f.render_widget(block, area);
        let ph = vec![
            Line::from(""),
            Line::from(Span::styled("  ┌──────────┐", Style::default().fg(Color::Rgb(35, 35, 35)))),
            Line::from(Span::styled("  │          │", Style::default().fg(Color::Rgb(35, 35, 35)))),
            Line::from(Span::styled("  │    ◆     │", Style::default().fg(crate::ui::accent()))),
            Line::from(Span::styled("  │          │", Style::default().fg(Color::Rgb(35, 35, 35)))),
            Line::from(Span::styled("  └──────────┘", Style::default().fg(Color::Rgb(35, 35, 35)))),
        ];
        f.render_widget(Paragraph::new(ph), inner);
    }
}

fn draw_detail_meta(
    f: &mut Frame,
    entry: &crate::db::history::HistoryEntry,
    area: Rect,
    focused: bool,
    episodes_focused: bool,
) {
    let max_w = (area.width as usize).saturating_sub(4);
    let mut lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            trunc(&entry.title, max_w),
            Style::default().fg(C_TEXT).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("  TYPE    ", Style::default().fg(C_DIM)),
            Span::styled(
                entry.media_type.to_uppercase(),
                Style::default().fg(crate::ui::accent()).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("  PLAYED  ", Style::default().fg(C_DIM)),
            Span::styled(
                format!("{}×", entry.play_count),
                Style::default().fg(C_TEXT).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("  LAST    ", Style::default().fg(C_DIM)),
            Span::styled(
                entry.last_watched.format("%Y-%m-%d  %H:%M").to_string(),
                Style::default().fg(C_TEXT),
            ),
        ]),
    ];

    match (entry.progress, entry.total) {
        (Some(p), Some(t)) => lines.push(Line::from(vec![
            Span::styled("  PROG    ", Style::default().fg(C_DIM)),
            Span::styled(format!("Ep {p} / {t}"), Style::default().fg(crate::ui::bar_complete()).add_modifier(Modifier::BOLD)),
        ])),
        (Some(p), None) => lines.push(Line::from(vec![
            Span::styled("  PROG    ", Style::default().fg(C_DIM)),
            Span::styled(format!("Ep {p}"), Style::default().fg(crate::ui::bar_complete()).add_modifier(Modifier::BOLD)),
        ])),
        _ => {}
    }

    if let Some(rating) = entry.user_rating {
        lines.push(Line::from(vec![
            Span::styled("  RATING  ", Style::default().fg(C_DIM)),
            Span::styled(
                format!("★ {:.1}", rating),
                Style::default().fg(Color::Rgb(255, 200, 0)).add_modifier(Modifier::BOLD),
            ),
        ]));
    }

    lines.push(Line::from(""));

    // Action hints — context-aware
    if focused {
        lines.push(Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(" [→] EPISODES ", Style::default()
                .fg(Color::Rgb(0, 0, 0)).bg(crate::ui::accent()).add_modifier(Modifier::BOLD)),
            Span::styled("   ", Style::default()),
            Span::styled("[Del] remove", Style::default().fg(C_DIM)),
            Span::styled("   ", Style::default()),
            Span::styled("[←] back", Style::default().fg(crate::ui::accent_dim())),
        ]));
    } else if episodes_focused {
        lines.push(Line::from(Span::styled(
            "  [↑↓] navigate  [Enter] play  [←] back to detail",
            Style::default().fg(C_DIM),
        )));
    } else {
        lines.push(Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled("[→] EPISODES", Style::default().fg(C_DIM)),
            Span::styled("   ", Style::default()),
            Span::styled("[Del] remove", Style::default().fg(C_DIM)),
        ]));
    }

    let block = Block::default()
        .title(Span::styled(
            " ENTRY ",
            Style::default()
                .fg(if focused { crate::ui::accent() } else { C_DIM })
                .add_modifier(if focused { Modifier::BOLD } else { Modifier::empty() }),
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(
            if focused { C_BORDER_F }
            else if episodes_focused { crate::ui::accent_dim() }
            else { C_BORDER }
        ))
        .style(Style::default().bg(C_PANEL));

    f.render_widget(Paragraph::new(lines).block(block), area);
}

fn draw_detail_gauge(f: &mut Frame, entry: &crate::db::history::HistoryEntry, area: Rect) {
    let pct = entry.progress_pct().unwrap_or(0.0);
    let label = if let (Some(p), Some(t)) = (entry.progress, entry.total) {
        format!("Ep {p} / {t}  —  {:.0}%", pct * 100.0)
    } else {
        "no progress tracked".to_string()
    };

    f.render_widget(
        Gauge::default()
            .block(Block::default()
                .title(Span::styled(" PROGRESS ", Style::default().fg(C_DIM)))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(C_BORDER))
                .style(Style::default().bg(C_PANEL)))
            .gauge_style(Style::default().fg(crate::ui::accent()).bg(Color::Rgb(20, 20, 20)))
            .ratio(pct)
            .label(Span::styled(
                label,
                Style::default().fg(Color::Rgb(0, 0, 0)).add_modifier(Modifier::BOLD),
            )),
        area,
    );
}

// ── Episode list ──────────────────────────────────────────────────────────────

fn draw_episode_list(
    f: &mut Frame,
    app: &mut App,
    entry: &crate::db::history::HistoryEntry,
    area: Rect,
    focused: bool,
) {
    let ep_count = app.history_episode_list.len();
    let loading  = app.history_episodes_loading;

    let title = if loading {
        Span::styled(
            format!(" {} EPISODES ", app.spinner.symbol()),
            Style::default().fg(crate::ui::accent()).add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled(
            format!(" EPISODES  {} ", ep_count),
            Style::default()
                .fg(if focused { crate::ui::accent() } else { C_DIM })
                .add_modifier(Modifier::BOLD),
        )
    };

    let container = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(if focused { C_BORDER_F } else { C_BORDER }))
        .style(Style::default().bg(C_PANEL));

    let inner = container.inner(area);
    f.render_widget(container, area);

    if ep_count == 0 { return; }

    // ── Adaptive column count — sync back to app for key navigation ───────────
    let cols: usize = if inner.width >= 120 { 3 } else { 2 };
    app.history_ep_cols = cols;

    // Each cell: bordered box — 4 rows tall (2 border + 1 label + 1 bar)
    let cell_h: u16 = 4;
    let visible_rows = (inner.height / cell_h) as usize;
    if visible_rows == 0 { return; }
    let visible_cells = visible_rows * cols;

    // Cell width — distribute evenly, last col takes remainder
    let col_w      = inner.width / cols as u16;
    let last_col_w = inner.width - col_w * (cols as u16 - 1);

    let sel_idx = app.history_episode_idx;
    let sel_row = sel_idx / cols;

    // Scroll: keep selected row in view
    let scroll_row = if sel_row >= visible_rows {
        sel_row - (visible_rows - 1)
    } else {
        0
    };
    let start_idx = scroll_row * cols;
    let last_watched = entry.progress.map(|p| p.to_string());

    for slot in 0..visible_cells {
        let list_pos = start_idx + slot;
        if list_pos >= ep_count { break; }

        let ep_str  = &app.history_episode_list[list_pos].clone();
        let rec     = app.history_ep_window_records.get(ep_str.as_str());
        let sel     = list_pos == sel_idx;
        let is_cur  = last_watched.as_deref() == Some(ep_str.as_str());

        let grid_row = slot / cols;
        let grid_col = slot % cols;

        let cell_x = inner.x + (grid_col as u16 * col_w);
        let cell_w = if grid_col == cols - 1 { last_col_w } else { col_w };
        let cell_y = inner.y + (grid_row as u16 * cell_h);

        if cell_y + cell_h > inner.y + inner.height { break; }

        let cell_bg = if sel { C_BG3 } else { C_BG };

        // ── Bordered cell ─────────────────────────────────────────────────────
        let border_style = if sel && focused {
            Style::default().fg(crate::ui::accent()).add_modifier(Modifier::BOLD)
        } else if sel {
            Style::default().fg(C_TEXT)
        } else if is_cur {
            Style::default().fg(crate::ui::accent_dim())
        } else {
            Style::default().fg(Color::Rgb(32, 32, 32))
        };

        let cell_block = Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .style(Style::default().bg(cell_bg));

        let cell_inner = cell_block.inner(Rect { x: cell_x, y: cell_y, width: cell_w, height: cell_h });
        f.render_widget(cell_block, Rect { x: cell_x, y: cell_y, width: cell_w, height: cell_h });

        // ── Label line ────────────────────────────────────────────────────────
        let (icon, icon_color) = match rec {
            Some(r) if r.fully_watched          => ("✓", crate::ui::bar_complete()),
            Some(r) if r.position_seconds > 5.0 => ("▶", crate::ui::accent()),
            _                                    => ("·", C_DIM),
        };

        let label_style = if sel && focused {
            Style::default().fg(crate::ui::accent()).add_modifier(Modifier::BOLD)
        } else if sel || is_cur {
            Style::default().fg(C_TEXT).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(C_TEXT)
        };

        let max_label = (cell_inner.width as usize).saturating_sub(5);
        let ep_label  = crate::ui::trunc(&format!("Ep {ep_str}"), max_label);

        let mut label_spans = vec![
            Span::styled(icon, Style::default().fg(icon_color).bg(cell_bg)),
            Span::styled(" ", Style::default().bg(cell_bg)),
            Span::styled(ep_label, label_style.bg(cell_bg)),
        ];

        if is_cur && !sel {
            label_spans.push(Span::styled(" ◀", Style::default().fg(crate::ui::accent()).bg(cell_bg)));
        }

        // Resume timestamp on selected focused cell
        if sel && focused {
            let pos = rec.map(|r| r.position_seconds).unwrap_or(0.0);
            if pos > 5.0 {
                let m = (pos / 60.0) as u64;
                let s = (pos as u64) % 60;
                label_spans.push(Span::styled(
                    format!("  {m}:{s:02}"),
                    Style::default().fg(crate::ui::accent_dim()).bg(cell_bg),
                ));
            }
        }

        f.render_widget(
            Paragraph::new(Line::from(label_spans)).style(Style::default().bg(cell_bg)),
            Rect { x: cell_inner.x, y: cell_inner.y, width: cell_inner.width, height: 1 },
        );

        // ── Progress bar line ─────────────────────────────────────────────────
        let bar_y = cell_inner.y + 1;
        if bar_y >= inner.y + inner.height { continue; }

        let bar_color = if rec.map(|r| r.fully_watched).unwrap_or(false) { crate::ui::bar_complete() } else { crate::ui::bar_progress() };

        // Compute pct: exact ratio if we have duration, estimate if not
        let pct = rec.and_then(|r| {
            if r.fully_watched {
                Some(1.0f64)   // always show full bar for watched episodes
            } else if r.duration_seconds > 0.0 && r.position_seconds > 0.0 {
                Some((r.position_seconds / r.duration_seconds).clamp(0.0, 1.0))
            } else if r.position_seconds > 0.0 {
                // No duration yet — estimate using 24min typical episode, cap at 0.95
                Some((r.position_seconds / 1440.0).clamp(0.0, 0.95))
            } else {
                None
            }
        });

        let pct_label = pct.map(|p| format!("{:.0}%", p * 100.0));
        let pct_w     = pct_label.as_ref().map(|l| l.len() as u16 + 1).unwrap_or(0);
        let bar_w     = cell_inner.width.saturating_sub(pct_w);

        let bar_line = if let Some(p) = pct {
            let filled = (p * bar_w as f64).round() as usize;
            let empty  = (bar_w as usize).saturating_sub(filled);
            let mut spans = vec![
                Span::styled(
                    "█".repeat(filled),
                    Style::default().fg(bar_color).bg(cell_bg),
                ),
                Span::styled(
                    "░".repeat(empty),
                    Style::default().fg(Color::Rgb(45, 45, 45)).bg(cell_bg),
                ),
            ];
            if let Some(label) = pct_label {
                spans.push(Span::styled(
                    format!(" {label}"),
                    Style::default().fg(C_DIM).bg(cell_bg),
                ));
            }
            Line::from(spans)
        } else {
            // Unwatched — dim empty bar
            Line::from(Span::styled(
                "░".repeat(bar_w as usize),
                Style::default().fg(Color::Rgb(35, 35, 35)).bg(cell_bg),
            ))
        };

        f.render_widget(
            Paragraph::new(bar_line).style(Style::default().bg(cell_bg)),
            Rect { x: cell_inner.x, y: bar_y, width: cell_inner.width, height: 1 },
        );
    }

    // ── Scrollbar ─────────────────────────────────────────────────────────────
    if ep_count > visible_cells {
        let total_rows  = (ep_count + cols - 1) / cols;
        let scroll_max  = total_rows.saturating_sub(visible_rows);
        let pct         = if scroll_max > 0 { scroll_row as f64 / scroll_max as f64 } else { 0.0 };
        let track_h     = inner.height.saturating_sub(1);
        let thumb_y     = inner.y + (pct * track_h as f64) as u16;
        let track_x     = area.x + area.width.saturating_sub(1);

        for ty in (inner.y)..(inner.y + inner.height) {
            let (ch, style) = if ty == thumb_y {
                ("█", Style::default().fg(crate::ui::accent()))
            } else {
                ("│", Style::default().fg(Color::Rgb(35, 35, 35)))
            };
            f.render_widget(
                Paragraph::new(Span::styled(ch, style)),
                Rect { x: track_x, y: ty, width: 1, height: 1 },
            );
        }
    }
}
