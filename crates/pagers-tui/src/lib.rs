mod app;
mod event;
mod state;
mod stats;
mod ui;

use std::io::{self, Stdout};
use std::sync::Arc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use pagers_core::Cancellation;
use pagers_core::events::Event as CoreEvent;
use pagers_core::mincore::PageMap;
use pagers_core::ops::Stats;
use ratatui_core::buffer::Buffer;
use ratatui_core::layout::Rect;
use ratatui_core::terminal::{Terminal, TerminalOptions, Viewport};
use ratatui_core::widgets::Widget;
use ratatui_crossterm::{CrosstermBackend, crossterm};

use app::App;
use state::FileState;

const MAX_DISPLAY_FILES: u16 = 8;
const MAX_DISPLAY_PAGES: usize = 32;
const FRAME_BUDGET: Duration = Duration::from_millis(100);

struct TerminalGuard {
    terminal: Terminal<CrosstermBackend<Stdout>>,
    _state: TerminalStateGuard,
}

struct TerminalStateGuard;

impl TerminalStateGuard {
    fn new() -> io::Result<Self> {
        crossterm::terminal::enable_raw_mode()?;
        Ok(Self)
    }
}

impl Drop for TerminalStateGuard {
    fn drop(&mut self) {
        let _ = crossterm::execute!(io::stdout(), crossterm::cursor::Show);
        let _ = crossterm::terminal::disable_raw_mode();
    }
}

impl TerminalGuard {
    fn new(viewport_height: u16) -> io::Result<Self> {
        let state = TerminalStateGuard::new()?;
        crossterm::execute!(io::stdout(), crossterm::cursor::Hide)?;
        let backend = CrosstermBackend::new(io::stdout());
        let terminal = Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::Inline(viewport_height),
            },
        )?;
        Ok(Self {
            terminal,
            _state: state,
        })
    }
}

struct RenderContext<'a> {
    core_stats: &'a Stats,
    label: &'a str,
    action_sign: isize,
}

impl RenderContext<'_> {
    fn render<PM: PageMap>(
        &self,
        files: &[&FileState<PM>],
        file_rows_hwm: u16,
        elapsed: f64,
        area: Rect,
        buf: &mut Buffer,
    ) {
        let [files_area, stats_area] = ui::layout(file_rows_hwm, area);
        ui::FileListWidget {
            files,
            max_rows: file_rows_hwm,
        }
        .render(files_area, buf);
        stats::SummaryWidget {
            stats: self.core_stats,
            elapsed,
            label: self.label,
            action_sign: self.action_sign,
        }
        .render(stats_area, buf);
    }
}

fn drain_events<PM: PageMap>(
    app: &mut App<PM>,
    rx: &mpsc::Receiver<event::TuiEvent<PM>>,
) -> app::ControlFlow {
    while let Ok(evt) = rx.try_recv() {
        match app.handle_event(evt) {
            app::ControlFlow::Continue => {}
            flow => return flow,
        }
    }
    app::ControlFlow::Continue
}

pub fn run<PM: PageMap + Send + 'static>(
    rx: mpsc::Receiver<CoreEvent<PM>>,
    cancellation: Cancellation,
    core_stats: Arc<Stats>,
    label: &str,
    action_sign: isize,
    start: Instant,
) -> io::Result<()> {
    let viewport_height = MAX_DISPLAY_FILES + stats::SUMMARY_LINES;
    let mut guard = TerminalGuard::new(viewport_height)?;

    let tui_rx = event::spawn_event_threads(rx, cancellation.clone());
    let mut app = App::new();
    let mut file_rows_hwm: u16 = 0;
    let ctx = RenderContext {
        core_stats: &core_stats,
        label,
        action_sign,
    };

    let flow = loop {
        let flow = match tui_rx.recv_timeout(FRAME_BUDGET) {
            Ok(evt) => app.handle_event(evt),
            Err(mpsc::RecvTimeoutError::Timeout) => app::ControlFlow::Continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break app::ControlFlow::Quit,
        };

        let flow = match flow {
            app::ControlFlow::Continue => drain_events(&mut app, &tui_rx),
            other => other,
        };

        let elapsed = start.elapsed().as_secs_f64();
        let files = app.visible_files(MAX_DISPLAY_FILES as usize);
        file_rows_hwm = file_rows_hwm.max(files.len().min(MAX_DISPLAY_FILES as usize) as u16);
        guard.terminal.draw(|frame| {
            ctx.render(
                &files,
                file_rows_hwm,
                elapsed,
                frame.area(),
                frame.buffer_mut(),
            );
        })?;

        match flow {
            app::ControlFlow::Continue => {}
            other => break other,
        }
    };

    if matches!(flow, app::ControlFlow::Quit) {
        cancellation.cancel();
    }

    if matches!(flow, app::ControlFlow::Done) {
        let elapsed = start.elapsed().as_secs_f64();
        let files = app.visible_files(MAX_DISPLAY_FILES as usize);
        file_rows_hwm = file_rows_hwm.max(files.len().min(MAX_DISPLAY_FILES as usize) as u16);
        let total_lines = file_rows_hwm + stats::SUMMARY_LINES;

        let _ = guard.terminal.insert_before(total_lines, |buf| {
            ctx.render(&files, file_rows_hwm, elapsed, buf.area, buf);
        });
    }

    Ok(())
}
