mod api;
mod app;
mod config;
mod db;
mod player;
mod ui;

use anyhow::Result;
use app::{App, AppMsg};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::{io, time::Duration};

#[tokio::main]
async fn main() -> Result<()> {
    // Write sample config if first run
    let _ = config::Config::write_sample();

    // Create image picker BEFORE entering raw mode so the stdio query works correctly.
    // The picker probes the terminal for graphics protocol support (Kitty/Sixel/iTerm2)
    // by writing escape sequences to stdout and reading responses from stdin.
    let mut image_picker = ratatui_image::picker::Picker::from_query_stdio()
        .unwrap_or_else(|_| ratatui_image::picker::Picker::from_fontsize((8, 16)));

    // The library's iterm2_from_env() maps "vscode" → Iterm2, but VS Code's terminal
    // (xterm.js) actually supports Sixel, not iTerm2 inline images (OSC 1337).
    // Override to Sixel for VS Code so we get proper image rendering.
    let term_prog = std::env::var("TERM_PROGRAM").unwrap_or_default();

    // WezTerm fully supports the Kitty graphics protocol on all platforms
    // including Windows — force it to Kitty for best image quality.
    if term_prog.contains("WezTerm") {
        image_picker.set_protocol_type(ratatui_image::picker::ProtocolType::Kitty);
    } else if term_prog.contains("vscode") {
        image_picker.set_protocol_type(ratatui_image::picker::ProtocolType::Sixel);
    } else if image_picker.protocol_type() == ratatui_image::picker::ProtocolType::Halfblocks {
        // Safety net for other terminals: override Halfblocks → Iterm2 where supported.
        if term_prog.contains("iTerm")
            || term_prog.contains("mintty")
            || term_prog.contains("Tabby")
            || term_prog.contains("Hyper")
            || term_prog.contains("rio")
        {
            image_picker.set_protocol_type(ratatui_image::picker::ProtocolType::Iterm2);
        }
    }

    // Terminal setup
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(image_picker).await?;
    let result = run(&mut terminal, &mut app).await;

    // Always restore terminal, even on error
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(ref e) = result {
        eprintln!("\nnexus-tui error: {e}");
    }

    result
}

async fn run<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
) -> Result<()> {
    loop {
        if app.needs_redraw {
            terminal.clear()?;
            app.needs_redraw = false;
        }
        terminal.draw(|f| ui::draw(f, app))?;

        // Run mpv on the main thread — the only way to safely do terminal teardown/restore.
        // pending_mpv is set by handle_msg(LaunchMpv) or handle_msg(StreamUrl).
        if let Some((url, anime_id, episode, resume_from, skip_times, skip_setting)) =
            app.pending_mpv.take()
        {
            let db = app.db.clone();
            let tx = app.msg_tx.clone();

            crossterm::terminal::disable_raw_mode()?;
            crossterm::execute!(
                std::io::stdout(),
                crossterm::terminal::LeaveAlternateScreen,
                crossterm::event::DisableMouseCapture,
            )?;

            let (pb_tx, mut pb_rx) =
                tokio::sync::mpsc::unbounded_channel::<player::PlaybackEvent>();
            let tx_fwd = tx.clone();
            tokio::spawn(async move {
                while let Some(ev) = pb_rx.recv().await {
                    let _ = tx_fwd.send(AppMsg::Playback(ev));
                }
            });

            let (pos, dur) = player::launch_mpv_tracked(
                &url,
                &anime_id,
                &episode,
                resume_from,
                Some(pb_tx),
                skip_times,
                &skip_setting,
            )
            .unwrap_or((0.0, 0.0));

            // Restore ratatui
            crossterm::terminal::enable_raw_mode()?;
            crossterm::execute!(
                std::io::stdout(),
                crossterm::terminal::EnterAlternateScreen,
                crossterm::event::EnableMouseCapture,
            )?;

            // Save authoritative quit position
            if !anime_id.is_empty() && !episode.is_empty() {
                // If no watch_later file was written (natural end), force fully_watched
                // when position covers 80%+ of duration
                let effective_pos = if dur > 0.0 && pos > 0.0 && pos / dur >= 0.80 {
                    dur // treat as fully watched — store position = duration
                } else {
                    pos
                };
                let _ = db.update_position(&anime_id, &episode, effective_pos, dur);
                let _ = tx.send(AppMsg::Playback(player::PlaybackEvent::Finished {
                    anime_id,
                    episode,
                    position: effective_pos,
                    duration: dur,
                }));
            }

            terminal.clear()?;
            app.needs_redraw = true;
            continue;
        }

        // Poll for input (non-blocking — 100ms max)
        if event::poll(Duration::from_millis(100))? {
            match event::read()? {
                Event::Key(key) => {
                    if key.kind != KeyEventKind::Press {
                        continue;
                    }
                    if app.handle_key(key).await? {
                        return Ok(());
                    }
                }
                Event::Resize(_, _) => {
                    app.on_resize();
                }
                _ => {}
            }
        }

        // Background task tick (debounce, spinner, message drain)
        app.tick().await?;
    }
}
