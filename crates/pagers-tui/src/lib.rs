mod app;
mod event;
mod state;
mod ui;

pub use app::App;
pub use state::FileState;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;

use color_eyre::Result;
use pagers_core::events::Event as CoreEvent;
use ratatui::layout::{Constraint, Layout};
use ratatui::{TerminalOptions, Viewport};

/// Maximum number of file rows to display.
const MAX_DISPLAY_FILES: u16 = 16;

pub fn run(rx: mpsc::Receiver<CoreEvent>, term: Arc<AtomicBool>) -> Result<()> {
    color_eyre::install()?;

    let (_, term_height) = crossterm::terminal::size()?;
    let viewport_height = (term_height / 2).clamp(4, MAX_DISPLAY_FILES);

    let mut terminal = ratatui::init_with_options(TerminalOptions {
        viewport: Viewport::Inline(viewport_height),
    });

    let tui_rx = event::spawn_event_threads(rx, term);
    let mut app = App::new();
    let quit = AtomicBool::new(false);
    let done = AtomicBool::new(false);

    while let Ok(evt) = tui_rx.recv() {
        let mut needs_draw = matches!(evt, event::TuiEvent::Core(_));
        match app.handle_event(evt) {
            app::ControlFlow::Continue => {}
            app::ControlFlow::Done => {
                done.store(true, Ordering::Relaxed);
            }
            app::ControlFlow::Quit => {
                quit.store(true, Ordering::Relaxed);
            }
        }

        // Drain any queued events before drawing to batch updates
        if !done.load(Ordering::Relaxed) && !quit.load(Ordering::Relaxed) {
            while let Ok(next) = tui_rx.try_recv() {
                needs_draw |= matches!(next, event::TuiEvent::Core(_));
                match app.handle_event(next) {
                    app::ControlFlow::Continue => {}
                    app::ControlFlow::Done => {
                        done.store(true, Ordering::Relaxed);
                        break;
                    }
                    app::ControlFlow::Quit => {
                        quit.store(true, Ordering::Relaxed);
                        break;
                    }
                }
            }
        }

        if needs_draw {
            let files = app.files();
            terminal.draw(|frame| ui::render_refs(frame, &files, viewport_height))?;
        }

        if done.load(Ordering::Relaxed) || quit.load(Ordering::Relaxed) {
            break;
        }
    }

    ratatui::restore();

    if !(quit.load(Ordering::Relaxed) || done.load(Ordering::Relaxed)) {
        let files = app.into_files();
        let n = files.len().min(viewport_height as usize) as u16;
        if n > 0 {
            let _ = terminal.insert_before(n, |buf| {
                let areas =
                    Layout::vertical(files.iter().take(n as usize).map(|_| Constraint::Length(1)))
                        .split(buf.area);
                for (i, file) in files.iter().take(n as usize).enumerate() {
                    ui::render_file_row_to_buf(file, areas[i], buf);
                }
            });
        }
    }

    Ok(())
}
