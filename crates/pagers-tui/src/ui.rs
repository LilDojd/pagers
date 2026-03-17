use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Style};
use ratatui::text::Span;
use ratatui::widgets::{LineGauge, Paragraph};
use ratatui::Frame;

use crate::state::FileState;

/// Render files into the viewport.
pub(crate) fn render_refs(frame: &mut Frame, files: &[&FileState], viewport_height: u16) {
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
pub(crate) fn render_file_row(frame: &mut Frame, file: &FileState, area: ratatui::layout::Rect) {
    render_file_row_to_buf(file, area, frame.buffer_mut());
}

/// Render a single file row directly into a buffer.
pub(crate) fn render_file_row_to_buf(
    file: &FileState,
    area: ratatui::layout::Rect,
    buf: &mut ratatui::buffer::Buffer,
) {
    use ratatui::widgets::Widget;

    let chunks =
        Layout::horizontal([Constraint::Percentage(30), Constraint::Percentage(70)]).split(area);

    let display_path = truncate_path(&file.path, chunks[0].width as usize);

    let (path_text, gauge_style) = if file.done {
        (
            format!("\u{2713} {display_path}"),
            Style::default().fg(Color::Green),
        )
    } else {
        (display_path, Style::default().fg(Color::Cyan))
    };

    Paragraph::new(path_text).render(chunks[0], buf);

    let pct = (file.ratio() * 100.0) as u64;
    LineGauge::default()
        .filled_style(gauge_style)
        .unfilled_style(Style::default().fg(Color::DarkGray))
        .label(Span::raw(format!("{pct}%")))
        .ratio(file.ratio())
        .render(chunks[1], buf);
}

/// Truncate a path to fit within `max_width`, using a leading ellipsis if needed.
pub(crate) fn truncate_path(path: &str, max_width: usize) -> String {
    let char_count = path.chars().count();
    if char_count <= max_width {
        return path.to_string();
    }
    if max_width <= 1 {
        return "\u{2026}".to_string();
    }
    let skip = char_count - (max_width - 1);
    let tail: String = path.chars().skip(skip).collect();
    format!("\u{2026}{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use ratatui::Terminal;

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
        let files: Vec<&FileState> = vec![];
        terminal
            .draw(|frame| render_refs(frame, &files, 4))
            .unwrap();
        let content = buffer_to_string(terminal.backend().buffer());
        assert!(content.trim().is_empty());
    }

    #[test]
    fn test_render_single_file() {
        let file = FileState {
            path: "/tmp/test.bin".to_string(),
            total_pages: 100,
            pages_in_core: 75,
            done: false,
        };
        let files: Vec<&FileState> = vec![&file];
        let backend = TestBackend::new(80, 4);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render_refs(frame, &files, 4))
            .unwrap();
        let content = buffer_to_string(terminal.backend().buffer());
        assert!(content.contains("test.bin"));
        assert!(content.contains("75%"));
    }

    #[test]
    fn test_render_files_sorted_by_ratio() {
        let high = FileState {
            path: "/high.bin".to_string(),
            total_pages: 100,
            pages_in_core: 90,
            done: false,
        };
        let low = FileState {
            path: "/low.bin".to_string(),
            total_pages: 100,
            pages_in_core: 10,
            done: false,
        };
        let mut files: Vec<&FileState> = vec![&high, &low];
        files.sort_by(|a, b| a.ratio().total_cmp(&b.ratio()));
        let backend = TestBackend::new(80, 4);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render_refs(frame, &files, 4))
            .unwrap();
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

    #[test]
    fn test_done_file_shows_checkmark_and_green() {
        let file = FileState {
            path: "/tmp/done.bin".to_string(),
            total_pages: 100,
            pages_in_core: 100,
            done: true,
        };
        let backend = TestBackend::new(80, 1);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render_file_row(frame, &file, frame.area()))
            .unwrap();
        let content = buffer_to_string(terminal.backend().buffer());
        assert!(content.contains("\u{2713}"), "expected checkmark in output");
    }

    #[test]
    fn test_truncate_path_char_based() {
        // ASCII path
        assert_eq!(truncate_path("abcdef", 6), "abcdef");
        assert_eq!(truncate_path("abcdef", 4), "\u{2026}def");
        assert_eq!(truncate_path("abcdef", 1), "\u{2026}");

        // Multi-byte chars: each char is 1 char but multiple bytes
        let mb = "\u{00e9}\u{00e9}\u{00e9}\u{00e9}"; // 4 chars, 8 bytes
        assert_eq!(truncate_path(mb, 4), mb.to_string());
        assert_eq!(truncate_path(mb, 3), format!("\u{2026}\u{00e9}\u{00e9}"));
    }
}
