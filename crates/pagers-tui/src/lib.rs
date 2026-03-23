mod app;
mod event;
mod state;
mod stats;
mod ui;

pub use app::App;
pub use state::FileState;

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use color_eyre::Result;
use pagers_core::events::Event as CoreEvent;
use pagers_core::mincore::PageMap;
use pagers_core::ops::Stats;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::{TerminalOptions, Viewport};

/// Maximum number of file rows to display.
const MAX_DISPLAY_FILES: u16 = 8;
const MAX_DISPLAY_PAGES: usize = 32;

pub fn run<PM: PageMap + Send + 'static>(
    rx: mpsc::Receiver<CoreEvent<PM>>,
    term: Arc<AtomicBool>,
    core_stats: Arc<Stats>,
    label: &str,
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

    let term_cleanup = Arc::clone(&term);
    let tui_rx = event::spawn_event_threads(rx, term);
    let mut app = App::new();
    let mut quit = false;
    let mut done = false;

    loop {
        match tui_rx.recv_timeout(Duration::from_millis(100)) {
            Ok(evt) => {
                match app.handle_event(evt) {
                    app::ControlFlow::Continue => {}
                    app::ControlFlow::Done => done = true,
                    app::ControlFlow::Quit => quit = true,
                }

                if !done && !quit {
                    while let Ok(next) = tui_rx.try_recv() {
                        match app.handle_event(next) {
                            app::ControlFlow::Continue => {}
                            app::ControlFlow::Done => {
                                done = true;
                                break;
                            }
                            app::ControlFlow::Quit => {
                                quit = true;
                                break;
                            }
                        }
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }

        let elapsed = start.elapsed().as_secs_f64();
        let files = app.visible_files(MAX_DISPLAY_FILES as usize);
        terminal.draw(|frame| {
            ui::render_viewport(
                &files,
                MAX_DISPLAY_FILES,
                &core_stats,
                elapsed,
                label,
                frame.area(),
                frame.buffer_mut(),
            );
        })?;

        if done || quit {
            break;
        }
    }

    term_cleanup.store(true, std::sync::atomic::Ordering::Relaxed);

    if !quit {
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
                label,
                buf.area,
                buf,
            );
        });
    }

    crossterm::execute!(std::io::stdout(), crossterm::cursor::Show)?;
    crossterm::terminal::disable_raw_mode()?;

    Ok(())
}
