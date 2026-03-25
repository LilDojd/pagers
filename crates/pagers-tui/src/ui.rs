use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use pagers_core::mincore::PageMap;

use crate::MAX_DISPLAY_PAGES;
use crate::state::FileState;
use crate::stats;

pub(crate) fn layout(file_rows: u16, area: Rect) -> [Rect; 2] {
    Layout::vertical([
        Constraint::Length(file_rows),
        Constraint::Length(stats::SUMMARY_LINES),
    ])
    .areas(area)
}

pub(crate) struct FileListWidget<'a, PM: PageMap> {
    pub files: &'a [&'a FileState<PM>],
    pub max_rows: u16,
}

impl<PM: PageMap> Widget for FileListWidget<'_, PM> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if self.files.is_empty() {
            return;
        }
        let visible = &self.files[..self.files.len().min(self.max_rows as usize)];
        let constraints: Vec<Constraint> = visible.iter().map(|_| Constraint::Length(1)).collect();
        let areas = Layout::vertical(constraints).split(area);
        for (i, file) in visible.iter().enumerate() {
            FileRowWidget { file }.render(areas[i], buf);
        }
    }
}

struct FileRowWidget<'a, PM: PageMap> {
    file: &'a FileState<PM>,
}

impl<PM: PageMap> FileRowWidget<'_, PM> {
    fn status_and_style(&self) -> (&'static str, Style) {
        let fully_loaded =
            self.file.total_pages > 0 && self.file.pages_in_core == self.file.total_pages;
        if self.file.done && fully_loaded {
            ("\u{2713} ", Style::default().fg(Color::Green))
        } else if !self.file.done {
            ("  ", Style::default().fg(Color::Cyan))
        } else {
            ("  ", Style::default())
        }
    }

    fn histogram_spans(&self, width: usize, style: Style) -> Vec<Span<'static>> {
        let buckets = self.file.bucketize(width);
        let mut spans = Vec::with_capacity(width + 2);
        spans.push(Span::styled("[", Style::default().fg(Color::DarkGray)));
        for &(cached, total) in &buckets {
            let ratio = if total == 0 {
                0.0
            } else {
                cached as f64 / total as f64
            };
            let (ch, s) = match ratio {
                r if r >= 1.0 => ("#", style),
                r if r >= 0.75 => ("+", style),
                r if r >= 0.50 => ("=", Style::default().fg(Color::Cyan)),
                r if r >= 0.25 => ("-", Style::default().fg(Color::DarkGray)),
                r if r > 0.0 => (".", Style::default().fg(Color::DarkGray)),
                _ => (" ", Style::default()),
            };
            spans.push(Span::styled(ch, s));
        }
        spans.push(Span::styled("]", Style::default().fg(Color::DarkGray)));
        spans
    }
}

impl<PM: PageMap> Widget for FileRowWidget<'_, PM> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let (status, cached_style) = self.status_and_style();
        let counter = format!(" {}/{}", self.file.pages_in_core, self.file.total_pages);

        let map_inner = self.file.total_pages.min(MAX_DISPLAY_PAGES);
        let map_chars = if map_inner > 0 { map_inner + 3 } else { 0 };
        let path_budget = (area.width as usize).saturating_sub(2 + map_chars + counter.len());
        let display_path = truncate_path(&self.file.path, path_budget);

        let mut spans = vec![
            Span::styled(status, cached_style),
            Span::styled(display_path, cached_style),
        ];

        if map_inner > 0 {
            spans.push(Span::raw(" "));
            spans.extend(self.histogram_spans(map_inner, cached_style));
        }

        spans.push(Span::styled(counter, Style::default().fg(Color::DarkGray)));

        Line::from(spans).render(area, buf);
    }
}

fn truncate_path(path: &str, max_width: usize) -> String {
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
    use bitvec::prelude::Lsb0;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

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

    fn render_file_list(files: &[&FileState], max_rows: u16, width: u16, height: u16) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                FileListWidget { files, max_rows }.render(frame.area(), frame.buffer_mut())
            })
            .unwrap();
        buffer_to_string(terminal.backend().buffer())
    }

    fn render_single_file(file: &FileState, width: u16) -> String {
        let backend = TestBackend::new(width, 1);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                FileRowWidget { file }.render(frame.area(), frame.buffer_mut())
            })
            .unwrap();
        buffer_to_string(terminal.backend().buffer())
    }

    #[test]
    fn test_render_empty_is_blank() {
        let files: Vec<&FileState> = vec![];
        let content = render_file_list(&files, 4, 60, 4);
        assert!(content.trim().is_empty());
    }

    #[test]
    fn test_render_single_file_shows_map_and_counter() {
        let file = FileState {
            path: "/tmp/test.bin".into(),
            total_pages: 100,
            pages_in_core: 75,
            residency: {
                let mut r = bitvec::bitvec![1; 75];
                r.extend(bitvec::bitvec![0; 25]);
                r
            },
            done: false,
        };
        let content = render_file_list(&[&file], 4, 80, 4);
        assert!(content.contains("test.bin"));
        assert!(content.contains("75/100"));
        assert!(content.contains('['));
        assert!(content.contains(']'));
    }

    #[test]
    fn test_render_files_sorted_by_ratio() {
        let high = FileState {
            path: "/high.bin".into(),
            total_pages: 100,
            pages_in_core: 90,
            residency: bitvec::bitvec![1; 100],
            done: false,
        };
        let low = FileState {
            path: "/low.bin".into(),
            total_pages: 100,
            pages_in_core: 10,
            residency: bitvec::bitvec![0; 100],
            done: false,
        };
        let mut files: Vec<&FileState> = vec![&high, &low];
        files.sort_by(|a, b| a.ratio().total_cmp(&b.ratio()));
        let backend = TestBackend::new(80, 4);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                FileListWidget {
                    files: &files,
                    max_rows: 4,
                }
                .render(frame.area(), frame.buffer_mut())
            })
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
            path: "test".into(),
            total_pages: 200,
            pages_in_core: 100,
            residency: bitvec::bitvec![1; 200],
            done: false,
        };
        assert!((f.ratio() - 0.5).abs() < f64::EPSILON);
        let empty = FileState {
            path: "empty".into(),
            total_pages: 0,
            pages_in_core: 0,
            residency: bitvec::bitvec![],
            done: false,
        };
        assert!((empty.ratio() - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_done_file_shows_checkmark() {
        let file = FileState {
            path: "/tmp/done.bin".into(),
            total_pages: 100,
            pages_in_core: 100,
            residency: bitvec::bitvec![1; 100],
            done: true,
        };
        let content = render_single_file(&file, 80);
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
        let mut residency = bitvec::bitvec![1; 50];
        residency.extend(bitvec::bitvec![0; 50]);
        let file = FileState {
            path: "/test.bin".into(),
            total_pages: 100,
            pages_in_core: 50,
            residency,
            done: false,
        };
        let content = render_single_file(&file, 80);
        assert!(content.contains('#') || content.contains('+'));
        assert!(content.contains("50/100"));
    }

    #[test]
    fn test_bucketize() {
        let file = FileState {
            path: "t".into(),
            total_pages: 10,
            pages_in_core: 5,
            residency: bitvec::bitvec![1, 1, 1, 1, 1, 0, 0, 0, 0, 0],
            done: false,
        };
        let buckets = file.bucketize(5);
        assert_eq!(buckets.len(), 5);
        assert_eq!(buckets[0], (2, 2));
        assert_eq!(buckets[1], (2, 2));
        assert_eq!(buckets[2], (1, 2));
        assert_eq!(buckets[3], (0, 2));
        assert_eq!(buckets[4], (0, 2));
    }

    #[test]
    fn test_bucketize_single_page_not_loaded() {
        let file = FileState {
            path: "t".into(),
            total_pages: 1,
            pages_in_core: 0,
            residency: bitvec::bitvec![0; 1],
            done: false,
        };
        let buckets = file.bucketize(1);
        assert_eq!(buckets.len(), 1);
        assert_eq!(buckets[0], (0, 1));
    }

    #[test]
    fn test_bucketize_single_page_loaded() {
        let file = FileState {
            path: "t".into(),
            total_pages: 1,
            pages_in_core: 1,
            residency: bitvec::bitvec![1],
            done: false,
        };
        let buckets = file.bucketize(1);
        assert_eq!(buckets.len(), 1);
        assert_eq!(buckets[0], (1, 1));
    }
}
