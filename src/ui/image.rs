use crate::app::App;
use crate::ui::C_DIM;
use ratatui::{
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

pub fn draw_cover(f: &mut Frame, app: &mut App, area: Rect) {
    if let Some(ref mut protocol) = app.cover_protocol {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Rgb(28, 28, 28)))
            .style(Style::default().bg(Color::Rgb(0, 0, 0)));
        let inner = block.inner(area);
        f.render_widget(block, area);

        // Each half-block cell = 1 col wide, 0.5 row tall in pixel terms.
        // We pre-compute the target pixel size so ratatui_image doesn't
        // over-shrink due to wrong cell-size assumptions.
        let cell_px_w: u32 = 8; // typical GNOME Terminal cell width in px
        let cell_px_h: u32 = 8; // half-block: each row = 2 half-rows, so treat as 8px
        let _target_w = (inner.width as u32).saturating_mul(cell_px_w);
        let _target_h = (inner.height as u32).saturating_mul(cell_px_h * 2);

        let image_widget = ratatui_image::StatefulImage::default().resize(
            ratatui_image::Resize::Fit(Some(image::imageops::FilterType::Lanczos3)),
        );

        f.render_stateful_widget(image_widget, inner, protocol);
        return;
    }

    let loading = app.is_searching || (app.selected.is_some() && app.cover_protocol.is_none());
    f.render_widget(
        Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled(
                "  ┌──────┐",
                Style::default().fg(Color::Rgb(35, 35, 35)),
            )),
            Line::from(Span::styled(
                "  │      │",
                Style::default().fg(Color::Rgb(35, 35, 35)),
            )),
            Line::from(Span::styled(
                "  │  ◆   │",
                Style::default().fg(crate::ui::accent()),
            )),
            Line::from(Span::styled(
                "  │      │",
                Style::default().fg(Color::Rgb(35, 35, 35)),
            )),
            Line::from(Span::styled(
                "  └──────┘",
                Style::default().fg(Color::Rgb(35, 35, 35)),
            )),
            Line::from(""),
            Line::from(Span::styled(
                if loading {
                    "  loading…"
                } else {
                    "  no cover"
                },
                Style::default().fg(C_DIM),
            )),
        ])
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Rgb(22, 22, 22)))
                .style(Style::default().bg(Color::Rgb(0, 0, 0))),
        ),
        area,
    );
}
