use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

use crate::MAX_DISPLAY_PAGES;
use crate::state::FileState;
use crate::stats;

/// Render file rows and stats summary into a buffer.
pub(crate) fn render_viewport(
    files: &[&FileState],
    max_file_rows: u16,
    core_stats: &pagers_core::ops::Stats,
    elapsed: f64,
    mode: &str,
    area: Rect,
    buf: &mut ratatui::buffer::Buffer,
) {
    let n = files.len().min(max_file_rows as usize) as u16;
    let [files_area, stats_area] = Layout::vertical([
        Constraint::Length(n),
        Constraint::Length(stats::SUMMARY_LINES),
    ])
    .areas(area);
    render_refs_to_buf(files, max_file_rows, files_area, buf);
    stats::render_summary(core_stats, elapsed, mode, stats_area, buf);
}

pub(crate) fn render_refs_to_buf(
    files: &[&FileState],
    max_rows: u16,
    area: ratatui::layout::Rect,
    buf: &mut ratatui::buffer::Buffer,
) {
    if files.is_empty() {
        return;
    }
    let visible = &files[..files.len().min(max_rows as usize)];
    let constraints: Vec<Constraint> = visible.iter().map(|_| Constraint::Length(1)).collect();
    let areas = Layout::vertical(constraints).split(area);
    for (i, file) in visible.iter().enumerate() {
        render_file_row_to_buf(file, areas[i], buf);
    }
}

pub(crate) fn render_file_row_to_buf(
    file: &FileState,
    area: ratatui::layout::Rect,
    buf: &mut ratatui::buffer::Buffer,
) {
    use ratatui::widgets::Widget;

    let fully_loaded = file.total_pages > 0 && file.pages_in_core == file.total_pages;
    let counter = format!(" {}/{}", file.pages_in_core, file.total_pages);

    let (status, cached_style) = if file.done && fully_loaded {
        ("\u{2713} ", Style::default().fg(Color::Green))
    } else if !file.done {
        ("  ", Style::default().fg(Color::Cyan))
    } else {
        ("  ", Style::default())
    };

    // Map width scales with total_pages, capped at 20
    let map_inner = file.total_pages.min(MAX_DISPLAY_PAGES);
    // brackets + space before map
    let map_chars = if map_inner > 0 { map_inner + 3 } else { 0 };
    let path_budget = (area.width as usize).saturating_sub(2 + map_chars + counter.len());
    let display_path = truncate_path(&file.path, path_budget);

    let mut spans = vec![
        Span::styled(status, cached_style),
        Span::styled(display_path, cached_style),
    ];

    if map_inner > 0 {
        let buckets = file.bucketize(map_inner);
        spans.push(Span::raw(" "));
        spans.push(Span::styled("[", Style::default().fg(Color::DarkGray)));
        for &(cached, total) in &buckets {
            let ratio = if total == 0 {
                0.0
            } else {
                cached as f64 / total as f64
            };
            let (ch, style) = match ratio {
                r if r >= 1.0 => ("#", cached_style),
                r if r >= 0.75 => ("+", cached_style),
                r if r >= 0.50 => ("=", Style::default().fg(Color::Cyan)),
                r if r >= 0.25 => ("-", Style::default().fg(Color::DarkGray)),
                r if r > 0.0 => (".", Style::default().fg(Color::DarkGray)),
                _ => (" ", Style::default()),
            };
            spans.push(Span::styled(ch, style));
        }
        spans.push(Span::styled("]", Style::default().fg(Color::DarkGray)));
    }

    spans.push(Span::styled(counter, Style::default().fg(Color::DarkGray)));

    Line::from(spans).render(area, buf);
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
    use ratatui::Terminal;
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
        let files: Vec<&FileState> = vec![];
        terminal
            .draw(|frame| render_refs_to_buf(&files, 4, frame.area(), frame.buffer_mut()))
            .unwrap();
        let content = buffer_to_string(terminal.backend().buffer());
        assert!(content.trim().is_empty());
    }

    #[test]
    fn test_render_single_file_shows_map_and_counter() {
        let file = FileState {
            path: "/tmp/test.bin".to_string(),
            total_pages: 100,
            pages_in_core: 75,
            residency: {
                let mut r = vec![true; 75];
                r.extend(vec![false; 25]);
                r
            },
            done: false,
        };
        let files: Vec<&FileState> = vec![&file];
        let backend = TestBackend::new(80, 4);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render_refs_to_buf(&files, 4, frame.area(), frame.buffer_mut()))
            .unwrap();
        let content = buffer_to_string(terminal.backend().buffer());
        assert!(content.contains("test.bin"));
        assert!(content.contains("75/100"));
        assert!(content.contains('['));
        assert!(content.contains(']'));
    }

    #[test]
    fn test_render_files_sorted_by_ratio() {
        let high = FileState {
            path: "/high.bin".to_string(),
            total_pages: 100,
            pages_in_core: 90,
            residency: vec![true; 100], // simplified
            done: false,
        };
        let low = FileState {
            path: "/low.bin".to_string(),
            total_pages: 100,
            pages_in_core: 10,
            residency: vec![false; 100],
            done: false,
        };
        let mut files: Vec<&FileState> = vec![&high, &low];
        files.sort_by(|a, b| a.ratio().total_cmp(&b.ratio()));
        let backend = TestBackend::new(80, 4);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render_refs_to_buf(&files, 4, frame.area(), frame.buffer_mut()))
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
            residency: vec![true; 200],
            done: false,
        };
        assert!((f.ratio() - 0.5).abs() < f64::EPSILON);
        let empty = FileState {
            path: "empty".to_string(),
            total_pages: 0,
            pages_in_core: 0,
            residency: vec![],
            done: false,
        };
        assert!((empty.ratio() - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_done_file_shows_checkmark() {
        let file = FileState {
            path: "/tmp/done.bin".to_string(),
            total_pages: 100,
            pages_in_core: 100,
            residency: vec![true; 100],
            done: true,
        };
        let backend = TestBackend::new(80, 1);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render_file_row_to_buf(&file, frame.area(), frame.buffer_mut()))
            .unwrap();
        let content = buffer_to_string(terminal.backend().buffer());
        assert!(content.contains("\u{2713}"), "expected checkmark in output");
    }

    #[test]
    fn test_truncate_path_char_based() {
        assert_eq!(truncate_path("abcdef", 6), "abcdef");
        assert_eq!(truncate_path("abcdef", 4), "\u{2026}def");
        assert_eq!(truncate_path("abcdef", 1), "\u{2026}");

        let mb = "\u{00e9}\u{00e9}\u{00e9}\u{00e9}";
        assert_eq!(truncate_path(mb, 4), mb.to_string());
        assert_eq!(truncate_path(mb, 3), format!("\u{2026}\u{00e9}\u{00e9}"));
    }

    #[test]
    fn test_page_map_shows_cached_regions() {
        // First half cached, second half not
        let mut residency = vec![true; 50];
        residency.extend(vec![false; 50]);
        let file = FileState {
            path: "/test.bin".to_string(),
            total_pages: 100,
            pages_in_core: 50,
            residency,
            done: false,
        };
        let backend = TestBackend::new(80, 1);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render_file_row_to_buf(&file, frame.area(), frame.buffer_mut()))
            .unwrap();
        let content = buffer_to_string(terminal.backend().buffer());
        // Should contain bar chars for cached region and spaces for uncached
        assert!(content.contains('#') || content.contains('+'));
        assert!(content.contains("50/100"));
    }

    #[test]
    fn test_bucketize() {
        let file = FileState {
            path: "t".to_string(),
            total_pages: 10,
            pages_in_core: 5,
            residency: vec![
                true, true, true, true, true, false, false, false, false, false,
            ],
            done: false,
        };
        let buckets = file.bucketize(5);
        assert_eq!(buckets.len(), 5);
        // First 2-3 buckets should be fully cached, rest not
        assert_eq!(buckets[0], (2, 2));
        assert_eq!(buckets[1], (2, 2));
        assert_eq!(buckets[2], (1, 2));
        assert_eq!(buckets[3], (0, 2));
        assert_eq!(buckets[4], (0, 2));
    }

    #[test]
    fn test_bucketize_single_page_not_loaded() {
        let file = FileState {
            path: "t".to_string(),
            total_pages: 1,
            pages_in_core: 0,
            residency: vec![false],
            done: false,
        };
        let buckets = file.bucketize(1);
        assert_eq!(buckets.len(), 1);
        assert_eq!(buckets[0], (0, 1));
    }

    #[test]
    fn test_bucketize_single_page_loaded() {
        let file = FileState {
            path: "t".to_string(),
            total_pages: 1,
            pages_in_core: 1,
            residency: vec![true],
            done: false,
        };
        let buckets = file.bucketize(1);
        assert_eq!(buckets.len(), 1);
        assert_eq!(buckets[0], (1, 1));
    }
}
