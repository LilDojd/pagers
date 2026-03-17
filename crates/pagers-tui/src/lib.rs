mod app;
mod event;
mod state;
mod ui;

pub use app::App;
pub use state::FileState;

use std::sync::mpsc;

use color_eyre::Result;
use pagers_core::events::Event as CoreEvent;
use ratatui::layout::{Constraint, Layout};
use ratatui::{TerminalOptions, Viewport};

/// Public entry point: creates an inline terminal and runs the event loop.
pub fn run(rx: mpsc::Receiver<CoreEvent>) -> Result<()> {
    let (_, term_height) = crossterm::terminal::size()?;
    let viewport_height = (term_height / 2).clamp(4, 16);

    let mut terminal = ratatui::init_with_options(TerminalOptions {
        viewport: Viewport::Inline(viewport_height),
    });

    let tui_rx = event::spawn_event_threads(rx);
    let mut app = App::new();
    let mut quit = false;

    while let Ok(evt) = tui_rx.recv() {
        let is_core = matches!(evt, event::TuiEvent::Core(_));
        match app.handle_event(evt) {
            app::ControlFlow::Continue => {
                if is_core {
                    let files = app.files();
                    terminal.draw(|frame| {
                        ui::render_refs(frame, &files, viewport_height)
                    })?;
                }
            }
            app::ControlFlow::Done => break,
            app::ControlFlow::Quit => {
                quit = true;
                break;
            }
        }
    }

    // Push final frame into scrollback so it survives restore.
    if !quit {
        let files = app.into_files();
        let n = files.len().min(viewport_height as usize) as u16;
        if n > 0 {
            let _ = terminal.insert_before(n, |buf| {
                let areas = Layout::vertical(
                    files.iter().take(n as usize).map(|_| Constraint::Length(1)),
                )
                .split(buf.area);
                for (i, file) in files.iter().take(n as usize).enumerate() {
                    ui::render_file_row_to_buf(file, areas[i], buf);
                }
            });
        }
    }

    ratatui::restore();
    Ok(())
}
