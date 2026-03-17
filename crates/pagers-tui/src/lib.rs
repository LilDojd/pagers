mod app;
mod event;
mod state;
mod ui;
pub use app::App;
pub use state::FileState;

use std::sync::mpsc;

use color_eyre::Result;
use ratatui::layout::{Constraint, Layout};
use ratatui::{Terminal, TerminalOptions, Viewport};

use event::TuiEvent;

/// Public entry point: creates an inline terminal and runs the event loop.
pub fn run(rx: mpsc::Receiver<pagers_core::events::Event>) -> Result<()> {
    let (_, term_height) = crossterm::terminal::size()?;
    let viewport_height = (term_height / 2).min(16).max(4);

    let mut terminal = ratatui::init_with_options(TerminalOptions {
        viewport: Viewport::Inline(viewport_height),
    });

    let result = run_loop(&mut terminal, rx, viewport_height);

    // Push final frame into scrollback so it survives restore.
    if let Ok((false, ref files)) = result {
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

    match result {
        Ok((true, _)) => std::process::exit(0),
        Ok((false, _)) => Ok(()),
        Err(e) => Err(e),
    }
}

/// Main event loop: receives events, updates state, redraws.
/// Returns (quit_requested, final_files).
fn run_loop<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    rx: mpsc::Receiver<pagers_core::events::Event>,
    viewport_height: u16,
) -> Result<(bool, Vec<FileState>)>
where
    B::Error: Send + Sync + 'static,
{
    let tui_rx = event::spawn_event_threads(rx);

    let mut files: Vec<FileState> = Vec::new();
    let mut needs_redraw = true;

    loop {
        if needs_redraw {
            files.sort_by(|a, b| a.ratio().partial_cmp(&b.ratio()).unwrap());
            terminal.draw(|frame| ui::render(frame, &files, viewport_height))?;
            needs_redraw = false;
        }

        match tui_rx.recv() {
            Ok(TuiEvent::Core(pagers_core::events::Event::FileStart {
                path,
                total_pages,
                residency,
            })) => {
                let pages_in_core = residency.iter().filter(|&&b| b).count();
                files.push(FileState {
                    path,
                    total_pages,
                    pages_in_core,
                    done: false,
                });
                needs_redraw = true;
            }
            Ok(TuiEvent::Core(pagers_core::events::Event::FileProgress { path, residency })) => {
                let pages_in_core = residency.iter().filter(|&&b| b).count();
                if let Some(f) = files.iter_mut().find(|f| f.path == path) {
                    f.pages_in_core = pages_in_core;
                    needs_redraw = true;
                }
            }
            Ok(TuiEvent::Core(pagers_core::events::Event::FileDone {
                path,
                pages_in_core,
                ..
            })) => {
                if let Some(f) = files.iter_mut().find(|f| f.path == path) {
                    f.pages_in_core = pages_in_core;
                    f.done = true;
                }
                needs_redraw = true;
            }
            Ok(TuiEvent::Tick) => {
                needs_redraw = true;
            }
            Ok(TuiEvent::CoreDone) | Err(_) => {
                files.sort_by(|a, b| a.ratio().partial_cmp(&b.ratio()).unwrap());
                return Ok((false, files));
            }
            Ok(TuiEvent::Quit) => {
                return Ok((true, files));
            }
        }
    }
}
