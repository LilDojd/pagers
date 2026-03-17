mod event;
mod state;
pub use state::FileState;

use std::sync::mpsc;

use color_eyre::Result;
use pagers_core::events::Event as CoreEvent;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Style};
use ratatui::text::Span;
use ratatui::widgets::{LineGauge, Paragraph};
use ratatui::{Frame, Terminal, TerminalOptions, Viewport};

use event::TuiEvent;

/// Public entry point: creates an inline terminal and runs the event loop.
pub fn run(rx: mpsc::Receiver<CoreEvent>) -> Result<()> {
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
                    render_file_row_to_buf(file, areas[i], buf);
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
    rx: mpsc::Receiver<CoreEvent>,
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
            terminal.draw(|frame| render(frame, &files, viewport_height))?;
            needs_redraw = false;
        }

        match tui_rx.recv() {
            Ok(TuiEvent::Core(CoreEvent::FileStart {
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
            Ok(TuiEvent::Core(CoreEvent::FileProgress { path, residency })) => {
                let pages_in_core = residency.iter().filter(|&&b| b).count();
                if let Some(f) = files.iter_mut().find(|f| f.path == path) {
                    f.pages_in_core = pages_in_core;
                    needs_redraw = true;
                }
            }
            Ok(TuiEvent::Core(CoreEvent::FileDone {
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

/// Render the full viewport.
fn render(frame: &mut Frame, files: &[FileState], viewport_height: u16) {
    if files.is_empty() {
        return;
    }

    let max_rows = viewport_height as usize;
    let visible = &files[..files.len().min(max_rows)];

    let constraints: Vec<Constraint> = visible.iter().map(|_| Constraint::Length(1)).collect();

    let areas = Layout::vertical(constraints).split(frame.area());

    for (i, file) in visible.iter().enumerate() {
        render_file_row(frame, file, areas[i]);
    }
}

/// Render a single file row: filename | LineGauge.
fn render_file_row(frame: &mut Frame, file: &FileState, area: ratatui::layout::Rect) {
    render_file_row_to_buf(file, area, frame.buffer_mut());
}

/// Render a single file row directly into a buffer.
fn render_file_row_to_buf(
    file: &FileState,
    area: ratatui::layout::Rect,
    buf: &mut ratatui::buffer::Buffer,
) {
    use ratatui::widgets::Widget;

    let chunks =
        Layout::horizontal([Constraint::Percentage(30), Constraint::Percentage(70)]).split(area);

    let display_path = truncate_path(&file.path, chunks[0].width as usize);
    Paragraph::new(display_path).render(chunks[0], buf);

    let pct = (file.ratio() * 100.0) as u64;
    LineGauge::default()
        .filled_style(Style::default().fg(Color::Cyan))
        .unfilled_style(Style::default().fg(Color::DarkGray))
        .label(Span::raw(format!("{pct}%")))
        .ratio(file.ratio())
        .render(chunks[1], buf);
}

/// Truncate a path to fit within `max_width`, using a leading "..." if needed.
fn truncate_path(path: &str, max_width: usize) -> String {
    if path.len() <= max_width {
        return path.to_string();
    }
    if max_width <= 1 {
        return "\u{2026}".to_string();
    }
    // Show the tail of the path with leading ellipsis.
    let tail = &path[path.len() - (max_width - 1)..];
    format!("\u{2026}{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;

    fn buffer_to_string(buf: &Buffer) -> String {
        let mut s = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                s.push_str(buf[(x, y)].symbol());
            }
            s.push('\n');
        }
        s
    }

    #[test]
    fn test_render_empty_is_blank() {
        let backend = TestBackend::new(60, 4);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| render(frame, &[], 4)).unwrap();
        let content = buffer_to_string(terminal.backend().buffer());
        assert!(content.trim().is_empty());
    }

    #[test]
    fn test_render_single_file() {
        let files = vec![FileState {
            path: "/tmp/test.bin".to_string(),
            total_pages: 100,
            pages_in_core: 75,
            done: false,
        }];
        let backend = TestBackend::new(80, 4);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| render(frame, &files, 4)).unwrap();
        let content = buffer_to_string(terminal.backend().buffer());
        assert!(content.contains("test.bin"));
        assert!(content.contains("75%"));
    }

    #[test]
    fn test_render_files_sorted_by_ratio() {
        let mut files = vec![
            FileState {
                path: "/high.bin".to_string(),
                total_pages: 100,
                pages_in_core: 90,
                done: false,
            },
            FileState {
                path: "/low.bin".to_string(),
                total_pages: 100,
                pages_in_core: 10,
                done: false,
            },
        ];
        files.sort_by(|a, b| a.ratio().partial_cmp(&b.ratio()).unwrap());
        let backend = TestBackend::new(80, 4);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| render(frame, &files, 4)).unwrap();
        let buf = terminal.backend().buffer().clone();
        let row0: String = (0..buf.area.width)
            .map(|x| buf[(x, 0)].symbol().chars().next().unwrap_or(' '))
            .collect();
        assert!(row0.contains("low.bin"));
    }

    #[test]
    fn test_file_state_ratio() {
        let f = FileState {
            path: "test".to_string(),
            total_pages: 200,
            pages_in_core: 100,
            done: false,
        };
        assert!((f.ratio() - 0.5).abs() < f64::EPSILON);
        let empty = FileState {
            path: "empty".to_string(),
            total_pages: 0,
            pages_in_core: 0,
            done: false,
        };
        assert!((empty.ratio() - 0.0).abs() < f64::EPSILON);
    }
}
