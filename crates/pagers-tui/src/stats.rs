use std::sync::atomic::Ordering;

use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Rect};
use ratatui::macros::{horizontal, vertical};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use pagers_core::ops::Stats;
use pagers_core::output::pretty_size;

pub(crate) const SUMMARY_LINES: u16 = 4;

const LABEL_WIDTH: u16 = 17;

pub(crate) fn render_summary(
    stats: &Stats,
    elapsed: f64,
    label: &str,
    area: Rect,
    buf: &mut Buffer,
) {
    let page_size = *pagers_core::pagesize::PAGE_SIZE as i64;
    let total_pages = stats.total_pages.load(Ordering::Relaxed);
    let pages_in_core = stats.total_pages_in_core.load(Ordering::Relaxed);
    let total_files = stats.total_files.load(Ordering::Relaxed);
    let total_dirs = stats.total_dirs.load(Ordering::Relaxed);

    let total_size = total_pages * page_size;
    let in_core_size = pages_in_core * page_size;

    let label_style = Style::default().fg(Color::DarkGray);

    let (pages_label, pages_value) = if label == "resident" {
        let pct = if total_pages > 0 {
            100.0 * pages_in_core as f64 / total_pages as f64
        } else {
            0.0
        };
        let mut spans = vec![Span::raw(format!(
            "{pages_in_core}/{total_pages}  {}/{}",
            pretty_size(in_core_size),
            pretty_size(total_size)
        ))];
        if total_pages > 0 {
            spans.push(Span::raw(format!("  {pct:.3}%")));
        }
        ("Resident Pages:".to_string(), Line::from(spans))
    } else {
        let mut cap = label.to_string();
        if let Some(c) = cap.get_mut(0..1) {
            c.make_ascii_uppercase();
        }
        (
            format!("{cap} Pages:"),
            Line::from(format!("{total_pages} ({})", pretty_size(total_size))),
        )
    };

    let rows: [(String, Line); 4] = [
        ("Files:".into(), Line::from(format!("{total_files}"))),
        ("Directories:".into(), Line::from(format!("{total_dirs}"))),
        (pages_label, pages_value),
        (
            "Elapsed:".into(),
            Line::from(format!("{elapsed:.5} seconds")),
        ),
    ];

    let row_areas = vertical![==1, ==1, ==1, ==1].split(area);
    for (i, (label_text, value_line)) in rows.into_iter().enumerate() {
        let [label_area, _, value_area] = horizontal![==LABEL_WIDTH, ==1, *=1].areas(row_areas[i]);

        Line::from(Span::styled(label_text, label_style))
            .alignment(Alignment::Right)
            .render(label_area, buf);

        value_line.render(value_area, buf);
    }
}
