use std::sync::atomic::Ordering;

use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use pagers_core::mmap;
use pagers_core::ops::Stats;
use pagers_core::output::{Mode, pretty_size};

pub(crate) const SUMMARY_LINES: u16 = 4;

pub(crate) fn render_summary(
    stats: &Stats,
    elapsed: f64,
    mode: Mode,
    area: Rect,
    buf: &mut Buffer,
) {
    let page_size = mmap::page_size() as i64;
    let total_pages = stats.total_pages.load(Ordering::Relaxed);
    let pages_in_core = stats.total_pages_in_core.load(Ordering::Relaxed);
    let total_files = stats.total_files.load(Ordering::Relaxed);
    let total_dirs = stats.total_dirs.load(Ordering::Relaxed);

    let total_size = total_pages * page_size;
    let in_core_size = pages_in_core * page_size;

    let label_style = Style::default().fg(Color::DarkGray);

    let pages_line = match mode {
        Mode::Touch => Line::from(vec![
            Span::styled("   Touched Pages: ", label_style),
            Span::raw(format!("{total_pages} ({})", pretty_size(total_size))),
        ]),
        Mode::Evict => Line::from(vec![
            Span::styled("   Evicted Pages: ", label_style),
            Span::raw(format!("{total_pages} ({})", pretty_size(total_size))),
        ]),
        _ => {
            let pct = if total_pages > 0 {
                100.0 * pages_in_core as f64 / total_pages as f64
            } else {
                0.0
            };
            let mut spans = vec![
                Span::styled("  Resident Pages: ", label_style),
                Span::raw(format!(
                    "{pages_in_core}/{total_pages}  {}/{}",
                    pretty_size(in_core_size),
                    pretty_size(total_size)
                )),
            ];
            if total_pages > 0 {
                spans.push(Span::raw(format!("  {pct:.3}%")));
            }
            Line::from(spans)
        }
    };

    let lines = [
        Line::from(vec![
            Span::styled("           Files: ", label_style),
            Span::raw(format!("{total_files}")),
        ]),
        Line::from(vec![
            Span::styled("     Directories: ", label_style),
            Span::raw(format!("{total_dirs}")),
        ]),
        pages_line,
        Line::from(vec![
            Span::styled("         Elapsed: ", label_style),
            Span::raw(format!("{elapsed:.5} seconds")),
        ]),
    ];

    let areas = Layout::vertical(lines.iter().map(|_| Constraint::Length(1))).split(area);
    for (i, line) in lines.into_iter().enumerate() {
        line.render(areas[i], buf);
    }
}
