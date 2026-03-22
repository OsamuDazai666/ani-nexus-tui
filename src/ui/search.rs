use crate::app::{App, Focus};
use crate::ui::{trunc, C_BG, C_BG2, C_BG3,
                C_BORDER_F, C_DIM, C_PANEL, C_SCORE, C_TEXT};
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

// ── Search bar ────────────────────────────────────────────────────────────────

pub fn draw_search_bar(f: &mut Frame, app: &App, area: Rect) {
    let focused = app.focus == Focus::Search;

    let content = if app.search_input.is_empty() && !focused {
        Line::from(Span::styled("  search…", Style::default().fg(C_DIM)))
    } else {
        Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(&app.search_input, Style::default().fg(C_TEXT).add_modifier(Modifier::BOLD)),
            Span::styled(
                if focused { "▌" } else { "" },
                Style::default().fg(crate::ui::accent()),
            ),
        ])
    };

    f.render_widget(
        Paragraph::new(content)
            .block(Block::default()
                .title(Span::styled(
                    if focused { " SEARCH " } else { " SEARCH [/] " },
                    Style::default()
                        .fg(if focused { crate::ui::accent() } else { C_DIM })
                        .add_modifier(if focused { Modifier::BOLD } else { Modifier::empty() }),
                ))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(if focused { C_BORDER_F } else { Color::Rgb(28,28,28) }))
                .style(Style::default().bg(if focused { C_BG3 } else { C_BG2 }))),
        area,
    );
}

// ── Results list ──────────────────────────────────────────────────────────────

pub fn draw_results(f: &mut Frame, app: &App, area: Rect) {
    let focused = app.focus == Focus::Results;

    // The main container box
    let title = match (app.results.len(), app.is_searching) {
        (_, true)  => format!(" {} SEARCHING ", app.spinner.symbol()),
        (0, false) => " RESULTS ".to_string(),
        (n, false) => format!(" RESULTS  {n}{} ", if app.has_more { "+" } else { "" }),
    };

    let container = Block::default()
        .title(Span::styled(title, Style::default()
            .fg(if focused { crate::ui::accent() } else { C_DIM })
            .add_modifier(Modifier::BOLD)))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(if focused { C_BORDER_F } else { Color::Rgb(28,28,28) }))
        .style(Style::default().bg(C_PANEL));
    
    let inner = container.inner(area);
    f.render_widget(container, area);

    if app.results.is_empty() { return; }

    // BOXED LAYOUT: Each item is a separate box (Block)
    let item_h = 4;
    let max_visible = (inner.height / item_h) as usize;
    if max_visible == 0 { return; }

    // Calculate scroll offset to keep results_idx in view
    let start_idx = if app.results_idx >= max_visible {
        app.results_idx - (max_visible - 1)
    } else {
        0
    };
    
    for (i, item) in app.results.iter().enumerate().skip(start_idx).take(max_visible) {
        let sel = i == app.results_idx;
        let score = item.score().map(|s| format!("{:.1}", s)).unwrap_or_default();
        let year  = item.year().map(|y| y.to_string()).unwrap_or_default();
        let eps = item.episodes_or_chapters().unwrap_or_default();

        let item_area = Rect {
            x: inner.x + 1,
            y: inner.y + ((i - start_idx) as u16 * item_h),
            width: inner.width.saturating_sub(2),
            height: 4, // 2 lines borders + 2 lines content
        };

        if item_area.y + item_area.height > inner.y + inner.height { break; }

        let block_style = if sel && focused {
            Style::default().fg(crate::ui::accent()).add_modifier(Modifier::BOLD)
        } else if sel {
            Style::default().fg(C_TEXT).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(C_DIM)
        };

        // Separate box (div inside div)
        let item_block = Block::default()
            .borders(Borders::ALL)
            .border_style(block_style)
            .style(Style::default().bg(if sel { C_BG3 } else { C_BG }));

        let content = Paragraph::new(vec![
            Line::from(vec![
                Span::styled(trunc(item.title(), (item_area.width as usize).saturating_sub(15)), 
                    if sel { Style::default().fg(crate::ui::accent()).add_modifier(Modifier::BOLD) } else { Style::default().fg(C_TEXT) }),
                Span::styled(if eps.is_empty() { "".to_string() } else { format!("  {}", eps) }, Style::default().fg(C_DIM)),
            ]),
            Line::from(vec![
                Span::styled(format!(" {}", year), Style::default().fg(C_DIM)),
                Span::styled("  ", Style::default()),
                Span::styled("★ ", Style::default().fg(C_SCORE)),
                Span::styled(format!("{}", score), Style::default().fg(C_SCORE)),
                Span::styled(format!("  {}", item.source_badge()), Style::default().fg(Color::Rgb(60,60,60))),
            ]),
        ]).block(item_block);

        f.render_widget(content, item_area);
    }
}
