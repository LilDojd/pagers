mod app;
mod event;
mod state;
mod stats;
mod ui;

pub use app::App;
pub use state::FileState;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Instant;

use color_eyre::Result;
use pagers_core::events::Event as CoreEvent;
use pagers_core::ops::Stats;
use pagers_core::output::Mode;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::{TerminalOptions, Viewport};

/// Maximum number of file rows to display.
const MAX_DISPLAY_FILES: u16 = 8;
const MAX_DISPLAY_PAGES: usize = 32;

pub fn run(
    rx: mpsc::Receiver<CoreEvent>,
    term: Arc<AtomicBool>,
    core_stats: Arc<Stats>,
    mode: Mode,
    start: Instant,
) -> Result<()> {
    color_eyre::install()?;

    let viewport_height = MAX_DISPLAY_FILES + stats::SUMMARY_LINES;

    crossterm::terminal::enable_raw_mode()?;
    crossterm::execute!(std::io::stdout(), crossterm::cursor::Hide)?;
    let backend = CrosstermBackend::new(std::io::stdout());
    let mut terminal = Terminal::with_options(
        backend,
        TerminalOptions {
            viewport: Viewport::Inline(viewport_height),
        },
    )?;

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
            let elapsed = start.elapsed().as_secs_f64();
            let files = app.visible_files(MAX_DISPLAY_FILES as usize);
            terminal.draw(|frame| {
                ui::render_viewport(
                    &files,
                    MAX_DISPLAY_FILES,
                    &core_stats,
                    elapsed,
                    mode,
                    frame.area(),
                    frame.buffer_mut(),
                );
            })?;
        }

        if done.load(Ordering::Relaxed) || quit.load(Ordering::Relaxed) {
            break;
        }
    }

    if !quit.load(Ordering::Relaxed) {
        let elapsed = start.elapsed().as_secs_f64();
        let files = app.visible_files(MAX_DISPLAY_FILES as usize);
        let n = files.len().min(MAX_DISPLAY_FILES as usize) as u16;
        let total_lines = n + stats::SUMMARY_LINES;

        let _ = terminal.insert_before(total_lines, |buf| {
            ui::render_viewport(
                &files,
                MAX_DISPLAY_FILES,
                &core_stats,
                elapsed,
                mode,
                buf.area,
                buf,
            );
        });
    }

    crossterm::execute!(std::io::stdout(), crossterm::cursor::Show)?;
    crossterm::terminal::disable_raw_mode()?;

    Ok(())
}
