//! Settings UI — clean linear row navigation.
//!
//! Layout:
//!   Left pane  (20 cols) — category list, j/k to navigate
//!   Right pane (rest)    — rows for selected category
//!
//! Navigation:
//!   j/k or ↑/↓         — move between rows in right pane
//!   ←/→                 — change value of current row
//!   Enter               — enter text-edit mode for text fields
//!   Ctrl+→ / Tab        — jump from category list → right pane
//!   Ctrl+← / Esc        — jump from right pane → category list
//!   Esc in category list — return to previous tab

use crate::app::{App, Focus};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};
use crate::ui::{C_BG, C_BG3, C_BORDER, C_BORDER_F, C_DIM, C_PANEL, C_TEXT};

const CATEGORIES: &[&str] = &["PLAYBACK", "THEME", "DISPLAY", "ABOUT"];

pub fn draw(f: &mut Frame, app: &App, area: Rect) {
    let cols = Layout::horizontal([
        Constraint::Length(20),
        Constraint::Min(0),
    ]).split(area);

    draw_category_list(f, app, cols[0]);
    draw_settings_pane(f, app, cols[1]);
}

// ── Left pane ─────────────────────────────────────────────────────────────────

fn draw_category_list(f: &mut Frame, app: &App, area: Rect) {
    let focused = app.focus == Focus::SettingsList;
    let accent  = crate::ui::accent();

    let block = Block::default()
        .title(Span::styled(" ⚙ SETTINGS ",
            Style::default().fg(if focused { accent } else { C_DIM }).add_modifier(Modifier::BOLD)))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(if focused { C_BORDER_F } else { C_BORDER }))
        .style(Style::default().bg(C_PANEL));

    let inner = block.inner(area);
    f.render_widget(block, area);

    // Hint at the bottom
    let hint_y = inner.y + inner.height.saturating_sub(1);
    f.render_widget(
        Paragraph::new(Span::styled(
            " [↑↓] nav  [→] open",
            Style::default().fg(Color::Rgb(45, 45, 45)),
        )),
        Rect { x: inner.x, y: hint_y, width: inner.width, height: 1 },
    );

    for (i, &cat) in CATEGORIES.iter().enumerate() {
        let sel = i == app.settings_category;
        let row_y = inner.y + (i as u16 * 3);
        if row_y + 3 > hint_y { break; }

        let row_area = Rect { x: inner.x, y: row_y, width: inner.width, height: 3 };

        let (bg, fg, border_col) = if sel && focused {
            (C_BG3, accent, accent)
        } else if sel {
            (C_BG3, C_TEXT, C_BORDER)
        } else {
            (C_BG, C_DIM, Color::Rgb(25, 25, 25))
        };

        let cat_block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_col))
            .style(Style::default().bg(bg));
        let cat_inner = cat_block.inner(row_area);
        f.render_widget(cat_block, row_area);

        let prefix = if sel { "▶ " } else { "  " };
        f.render_widget(
            Paragraph::new(Span::styled(
                format!("{prefix}{cat}"),
                Style::default().fg(fg)
                    .add_modifier(if sel { Modifier::BOLD } else { Modifier::empty() }),
            )).style(Style::default().bg(bg)),
            cat_inner,
        );
    }
}

// ── Right pane ────────────────────────────────────────────────────────────────

fn draw_settings_pane(f: &mut Frame, app: &App, area: Rect) {
    let edit    = app.focus == Focus::SettingsEdit;
    let accent  = crate::ui::accent();

    let block = Block::default()
        .title(Span::styled(
            format!(" {} ", CATEGORIES[app.settings_category]),
            Style::default().fg(accent).add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(if edit { C_BORDER_F } else { C_BORDER }))
        .style(Style::default().bg(C_PANEL));

    let inner = block.inner(area);
    f.render_widget(block, area);

    // Bottom hint bar
    let hint_y = area.y + area.height.saturating_sub(2);
    f.render_widget(
        Paragraph::new(Span::styled(
            "  [↑↓] move   [←→] change   [Enter] edit text   [Ctrl+←] back",
            Style::default().fg(Color::Rgb(45, 45, 45)),
        )),
        Rect { x: area.x + 1, y: hint_y, width: area.width.saturating_sub(2), height: 1 },
    );

    // Error
    if let Some(ref err) = app.settings_error {
        let err_y = hint_y.saturating_sub(1);
        f.render_widget(
            Paragraph::new(Span::styled(
                format!("  ✗ {err}"),
                Style::default().fg(Color::Rgb(255, 80, 80)),
            )),
            Rect { x: area.x + 1, y: err_y, width: area.width.saturating_sub(2), height: 1 },
        );
    }

    let content_area = Rect {
        x: inner.x, y: inner.y,
        width: inner.width,
        height: inner.height.saturating_sub(2),
    };

    match app.settings_category {
        0 => draw_playback(f, app, content_area, edit, accent),
        1 => draw_theme(f, app, content_area, edit, accent),
        2 => draw_display(f, app, content_area, edit, accent),
        3 => draw_about(f, app, content_area, accent),
        _ => {}
    }
}

// ── Row renderer ──────────────────────────────────────────────────────────────

/// Render a single settings row: label on left, value widget on right.
/// Returns the y position of the bottom of the row.
fn draw_row(
    f: &mut Frame,
    area: Rect,
    y_offset: u16,
    label: &str,
    value: Line,
    selected: bool,
    accent: Color,
) -> u16 {
    let row_h: u16 = 3;
    let row_area = Rect {
        x: area.x,
        y: area.y + y_offset,
        width: area.width,
        height: row_h,
    };
    if row_area.y + row_h > area.y + area.height {
        return y_offset + row_h;
    }

    let bg = if selected { C_BG3 } else { C_BG };
    let border_col = if selected { accent } else { Color::Rgb(30, 30, 30) };

    let row_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_col))
        .style(Style::default().bg(bg));
    let row_inner = row_block.inner(row_area);
    f.render_widget(row_block, row_area);

    let label_w = 22u16.min(row_inner.width / 3);
    let parts = Layout::horizontal([
        Constraint::Length(label_w),
        Constraint::Min(0),
    ]).split(row_inner);

    f.render_widget(
        Paragraph::new(Span::styled(
            label.to_string(),
            Style::default().fg(if selected { C_TEXT } else { C_DIM }),
        )).style(Style::default().bg(bg)),
        parts[0],
    );
    f.render_widget(
        Paragraph::new(value).style(Style::default().bg(bg)),
        parts[1],
    );

    y_offset + row_h
}

/// Render a multi-option toggle: [ opt1 ] [ opt2 ] with current highlighted.
fn toggle_line(current: &str, options: &[&str], accent: Color, selected: bool) -> Line<'static> {
    let mut spans = Vec::new();
    for &opt in options {
        let active = opt == current;
        let (bg, fg) = if active && selected {
            (accent, Color::Rgb(0, 0, 0))
        } else if active {
            (Color::Rgb(50, 50, 0), accent)
        } else {
            (Color::Rgb(22, 22, 22), C_DIM)
        };
        spans.push(Span::styled(
            format!(" {opt} "),
            Style::default().fg(fg).bg(bg)
                .add_modifier(if active { Modifier::BOLD } else { Modifier::empty() }),
        ));
        spans.push(Span::styled("  ", Style::default()));
    }
    Line::from(spans)
}

/// Render a color picker row: swatches inline, current one highlighted.
/// Calculate how many lines the color cells need to fit within `available_w`.
fn measure_color_row_lines(app: &App, row_idx: usize, available_w: u16) -> usize {
    let n_presets     = crate::config::COLOR_PRESET_NAMES.len() - 1;
    let customs       = app.color_customs(row_idx);
    let total         = n_presets + customs.len() + 1; // +1 for [+] cell
    let input_cell_w  = 12u16; // fixed — matches the input cell width used in draw_color_row

    let mut x: u16 = 0;
    let mut lines: usize = 1;

    for i in 0..total {
        let cell_w = if i < n_presets {
            crate::config::COLOR_PRESET_NAMES[i].len() as u16 + 4
        } else if i < n_presets + customs.len() {
            customs[i - n_presets].len().min(7) as u16 + 4
        } else {
            input_cell_w // [+] / input cell
        };
        if x + cell_w > available_w && x > 0 {
            lines += 1;
            x = 0;
        }
        x += cell_w + 1; // +1 gap
    }
    lines
}

/// Draw a full color picker row inline (handles wrapping to second line).
/// Returns total height used (1 or 2).
fn draw_color_row(
    f: &mut Frame,
    app: &App,
    area: Rect,          // inner area of the row block
    color_row_idx: usize,
    selected: bool,
    editing: bool,
    accent: Color,
) -> u16 {
    let n_presets  = crate::config::COLOR_PRESET_NAMES.len() - 1;
    let customs    = app.color_customs(color_row_idx);
    let cursor_idx = app.settings_color_idx[color_row_idx];
    let add_idx    = n_presets + customs.len();

    let cell_gap: u16 = 1;
    let mut x = area.x;
    let mut y = area.y;
    let max_x = area.x + area.width;

    let render_cell = |f: &mut Frame, label: &str, bg: Color, is_cur: bool,
                            is_active: bool, deletable: bool, x: &mut u16, y: &mut u16| {
        let cell_w = label.chars().count() as u16 + 2; // padding
        // Wrap if needed — cells are 3 tall so each new line is +3
        if *x + cell_w + cell_gap > max_x && *x > area.x {
            *y += 3;
            *x  = area.x;
        }
        if *y >= area.y + area.height { return; }

        let text_fg = if bg == Color::Rgb(28,28,28) || bg == Color::Rgb(40,40,40) {
            Color::Rgb(200,200,200)
        } else {
            // Dark text on bright bg
            Color::Rgb(0,0,0)
        };

        let border_col = if is_cur && selected {
            Color::Rgb(255,255,255)
        } else if is_active {
            accent
        } else {
            Color::Rgb(40,40,40)
        };

        let cell_block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_col))
            .style(Style::default().bg(bg));
        let cell_rect  = Rect { x: *x, y: *y, width: cell_w + 2, height: 3 };
        if cell_rect.x + cell_rect.width > max_x { return; }
        let cell_inner = cell_block.inner(cell_rect);
        f.render_widget(cell_block, cell_rect);

        let mut spans = vec![Span::styled(
            label.to_string(),
            Style::default().fg(text_fg).bg(bg)
                .add_modifier(if is_active { Modifier::BOLD } else { Modifier::empty() }),
        )];
        if deletable && is_cur && selected && !editing {
            spans.push(Span::styled(" ✕", Style::default().fg(Color::Rgb(180,50,50)).bg(bg)));
        }
        f.render_widget(
            Paragraph::new(Line::from(spans)).style(Style::default().bg(bg)),
            cell_inner,
        );

        *x += cell_w + 2 + cell_gap;
    };

    // ── Preset cells ──────────────────────────────────────────────────────────
    for (i, &name) in crate::config::COLOR_PRESET_NAMES[..n_presets].iter().enumerate() {
        let (r, g, b) = crate::config::Config::color_rgb(name);
        let bg        = Color::Rgb(r, g, b);
        let is_cur    = cursor_idx == i && selected;
        let is_active = {
            let active = match color_row_idx {
                0 => &app.config.theme.accent,
                1 => &app.config.theme.bar_progress,
                _ => &app.config.theme.bar_complete,
            };
            active == name
        };
        render_cell(f, name, bg, is_cur, is_active, false, &mut x, &mut y);
    }

    // ── Custom saved cells ────────────────────────────────────────────────────
    for (i, custom) in customs.iter().enumerate() {
        let ci = n_presets + i;
        let (r, g, b) = crate::config::Config::color_rgb(custom);
        let bg        = Color::Rgb(r, g, b);
        let is_cur    = cursor_idx == ci && selected;
        let is_active = {
            let active = match color_row_idx {
                0 => &app.config.theme.accent,
                1 => &app.config.theme.bar_progress,
                _ => &app.config.theme.bar_complete,
            };
            active == custom
        };
        let label = if custom.len() > 7 { &custom[..7] } else { custom };
        render_cell(f, label, bg, is_cur, is_active, true, &mut x, &mut y);
    }

    // ── [+] add cell / inline input ──────────────────────────────────────────
    let is_add_cur = cursor_idx == add_idx && selected;
    // Fixed width same as a typical color cell (keeps layout stable)
    let input_cell_w: u16 = 12; // matches "Yellow" cell width roughly

    if editing && is_add_cur {
        // Wrap check using the fixed cell width
        if x + input_cell_w > max_x && x > area.x { y += 3; x = area.x; }
        let cell_rect = Rect { x, y, width: input_cell_w, height: 3 };
        let border_col = if app.settings_error.is_some() {
            Color::Rgb(255, 80, 80)
        } else {
            accent
        };
        let input_block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_col))
            .style(Style::default().bg(Color::Rgb(18, 18, 18)));
        let input_inner = input_block.inner(cell_rect);
        f.render_widget(input_block, cell_rect);

        // Show input or grayed placeholder — truncated to cell width
        let inner_w = input_inner.width as usize;
        let (text, fg) = if app.settings_input.is_empty() {
            let hint = "#rrggbb▌";
            (hint[..hint.len().min(inner_w)].to_string(), Color::Rgb(55, 55, 55))
        } else {
            // Show tail of input so user sees what they're typing
            let with_cursor = format!("{}▌", app.settings_input);
            let chars: Vec<char> = with_cursor.chars().collect();
            let start = chars.len().saturating_sub(inner_w);
            (chars[start..].iter().collect(), accent)
        };
        f.render_widget(
            Paragraph::new(Span::styled(text,
                Style::default().fg(fg).add_modifier(Modifier::BOLD))),
            input_inner,
        );
    } else {
        // [+] button — same fixed width as the input cell so no jitter when switching
        let bg = if is_add_cur && selected { Color::Rgb(50, 50, 50) } else { Color::Rgb(25, 25, 25) };
        // Wrap check
        if x + input_cell_w > max_x && x > area.x { y += 3; x = area.x; }
        let cell_rect = Rect { x, y, width: input_cell_w, height: 3 };
        let border_col = if is_add_cur && selected {
            Color::Rgb(255, 255, 255)
        } else {
            Color::Rgb(40, 40, 40)
        };
        let btn_block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_col))
            .style(Style::default().bg(bg));
        let btn_inner = btn_block.inner(cell_rect);
        f.render_widget(btn_block, cell_rect);
        f.render_widget(
            Paragraph::new(Span::styled(
                "  +  ",
                Style::default().fg(Color::Rgb(120, 120, 120)).bg(bg)
                    .add_modifier(if is_add_cur && selected { Modifier::BOLD } else { Modifier::empty() }),
            )).style(Style::default().bg(bg)),
            btn_inner,
        );
    }

    (y - area.y) + 3  // total height used
}

/// Text field — shows value or edit cursor.
fn text_line(value: &str, editing: bool, input: &str, accent: Color) -> Line<'static> {
    if editing {
        Line::from(vec![
            Span::styled(
                format!("{input}▌"),
                Style::default().fg(accent).add_modifier(Modifier::BOLD),
            ),
        ])
    } else {
        Line::from(vec![
            Span::styled(value.to_string(), Style::default().fg(C_TEXT)),
            Span::styled("  [Enter to edit]", Style::default().fg(Color::Rgb(45, 45, 45))),
        ])
    }
}

// ── Category renderers ────────────────────────────────────────────────────────

fn draw_playback(f: &mut Frame, app: &App, area: Rect, edit: bool, accent: Color) {
    let cfg = &app.config.player;
    let mut y = 0u16;
    let sel = |r| edit && app.settings_row == r && app.settings_category == 0;

    y = draw_row(f, area, y, "Default audio",
        toggle_line(&cfg.stream_mode, &["sub","dub"], accent, sel(0)), sel(0), accent);
    y = draw_row(f, area, y, "Stream quality",
        toggle_line(&cfg.quality, &["best","1080","720","480"], accent, sel(1)), sel(1), accent);
    y = draw_row(f, area, y, "Skip segments",
        toggle_line(&cfg.skip_segments, &["none","intro","outro","both"], accent, sel(2)), sel(2), accent);

    // Resume offset — numeric stepper with ← / → arrows
    let offset_val = if cfg.resume_offset_secs == 0 {
        "off".to_string()
    } else {
        format!("{}s", cfg.resume_offset_secs)
    };
    let offset_line = {
        let s = sel(3);
        let col = if s { accent } else { Color::Rgb(80, 80, 80) };
        Line::from(vec![
            Span::styled("← ", Style::default().fg(col)),
            Span::styled(
                format!("{:>3}", offset_val),
                Style::default().fg(if s { Color::White } else { Color::Rgb(160,160,160) })
                    .add_modifier(if s { ratatui::style::Modifier::BOLD } else { ratatui::style::Modifier::empty() }),
            ),
            Span::styled(" →  ", Style::default().fg(col)),
            Span::styled("max 60s", Style::default().fg(Color::Rgb(50,50,50))),
        ])
    };
    y = draw_row(f, area, y, "Resume offset", offset_line, sel(3), accent);

    y = draw_row(f, area, y, "MPV path",
        text_line(&cfg.mpv_path, app.settings_editing && sel(4), &app.settings_input, accent),
        sel(4), accent);
    draw_row(f, area, y, "Extra MPV args",
        text_line(&cfg.extra_args.join(" "), app.settings_editing && sel(5), &app.settings_input, accent),
        sel(5), accent);
}

fn draw_theme(f: &mut Frame, app: &App, area: Rect, edit: bool, accent: Color) {
    let _cfg = &app.config.theme;
    let mut y = 0u16;
    let sel = |r| edit && app.settings_row == r && app.settings_category == 1;

    // ── Color rows with auto-expanding height ─────────────────────────────────
    let row_labels = ["Accent color", "Progress bar <90%", "Complete bar ≥90%"];
    let label_w = 22u16.min(area.width / 3);
    // 2 borders + cells need cell_area width = area.width - 2 (outer border) - label_w
    let cell_area_w = area.width.saturating_sub(2).saturating_sub(label_w);

    for row_idx in 0..3usize {
        let s = sel(row_idx);

        // ── Measure how many lines the cells need ──────────────────────────────
        let lines_needed = measure_color_row_lines(app, row_idx, cell_area_w);
        // Each line = 3 tall (cell height), plus 1px gap between lines
        let cells_h = lines_needed as u16 * 3 + (lines_needed as u16).saturating_sub(1);
        let row_h   = 2 + cells_h; // 2 for outer borders — never changes based on edit state

        let row_area = Rect {
            x: area.x, y: area.y + y,
            width: area.width, height: row_h,
        };
        if row_area.y + row_h > area.y + area.height { break; }

        let border_col = if s { accent } else { Color::Rgb(30,30,30) };
        let bg = if s { C_BG3 } else { C_BG };

        let outer = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_col))
            .style(Style::default().bg(bg));
        let inner = outer.inner(row_area);
        f.render_widget(outer, row_area);

        let parts = Layout::horizontal([
            Constraint::Length(label_w),
            Constraint::Min(0),
        ]).split(inner);

        f.render_widget(
            Paragraph::new(Span::styled(
                row_labels[row_idx],
                Style::default().fg(if s { C_TEXT } else { C_DIM }),
            )).style(Style::default().bg(bg)),
            parts[0],
        );

        draw_color_row(f, app, parts[1], row_idx, s, s && app.settings_editing, accent);

        y += row_h;

        // Error or delete hint renders in the 1px gap — no height impact
        if s {
            let hint_area = Rect { x: area.x + label_w + 2, y: area.y + y - 1,
                                   width: area.width.saturating_sub(label_w + 3), height: 1 };
            if let Some(ref err) = app.settings_error {
                f.render_widget(
                    Paragraph::new(Span::styled(err.clone(),
                        Style::default().fg(Color::Rgb(255, 80, 80)))),
                    hint_area,
                );
            } else {
                let n_presets = crate::config::COLOR_PRESET_NAMES.len() - 1;
                let cidx = app.settings_color_idx[row_idx];
                let n_customs = app.color_customs(row_idx).len();
                if !app.settings_editing && cidx >= n_presets && cidx < n_presets + n_customs {
                    f.render_widget(
                        Paragraph::new(Span::styled("  [Del] remove",
                            Style::default().fg(Color::Rgb(50, 50, 50)))),
                        hint_area,
                    );
                }
            }
        }
    }

    // ── Reset row ─────────────────────────────────────────────────────────────
    let reset_sel = sel(3);
    draw_row(f, area, y, "Reset all settings",
        Line::from(vec![
            Span::styled(
                " ↺ RESET TO DEFAULTS ",
                Style::default()
                    .fg(if reset_sel { Color::Rgb(0,0,0) } else { Color::Rgb(255,80,80) })
                    .bg(if reset_sel { Color::Rgb(255,80,80) } else { Color::Rgb(40,10,10) })
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        reset_sel, accent);

    // ── Static preview strip ──────────────────────────────────────────────────
    let preview_y = area.y + area.height.saturating_sub(3);
    if area.height > 20 {
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("  Accent → ", Style::default().fg(C_DIM)),
                Span::styled("████████", Style::default().fg(crate::ui::accent())),
                Span::styled("   Progress → ", Style::default().fg(C_DIM)),
                Span::styled("████████", Style::default().fg(crate::ui::bar_progress())),
                Span::styled("   Complete → ", Style::default().fg(C_DIM)),
                Span::styled("████████", Style::default().fg(crate::ui::bar_complete())),
            ])),
            Rect { x: area.x + 1, y: preview_y, width: area.width.saturating_sub(2), height: 1 },
        );
    }
}

fn draw_display(f: &mut Frame, app: &App, area: Rect, edit: bool, accent: Color) {
    let cfg = &app.config.ui;
    let mut y = 0u16;
    let sel = |r| edit && app.settings_row == r && app.settings_category == 2;

    y = draw_row(f, area, y, "Image protocol",
        toggle_line(&cfg.image_protocol, &["auto","kitty","halfblock"], accent, sel(0)),
        sel(0), accent);
    y = draw_row(f, area, y, "Results per page",
        toggle_line(&cfg.results_limit.to_string(), &["25","50"], accent, sel(1)),
        sel(1), accent);
    draw_row(f, area, y, "Episode columns",
        toggle_line(&cfg.episode_cols, &["auto","2","3"], accent, sel(2)),
        sel(2), accent);
}

fn draw_about(f: &mut Frame, _app: &App, area: Rect, accent: Color) {
    let lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("  ◆ ", Style::default().fg(accent).add_modifier(Modifier::BOLD)),
            Span::styled("nexus-tui", Style::default().fg(C_TEXT).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(""),
        Line::from(Span::styled("  A blazing-fast TUI anime client", Style::default().fg(C_DIM))),
        Line::from(""),
        Line::from(vec![
            Span::styled("  License   ", Style::default().fg(C_DIM)),
            Span::styled("GPL v3", Style::default().fg(C_TEXT)),
        ]),
        Line::from(vec![
            Span::styled("  Source    ", Style::default().fg(C_DIM)),
            Span::styled("github.com/nexus-tui", Style::default().fg(accent)),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "  [F1] Anime   [F2] History   [F3] Settings",
            Style::default().fg(C_DIM),
        )),
    ];
    f.render_widget(
        Paragraph::new(lines).style(Style::default().bg(C_PANEL)),
        area,
    );
}