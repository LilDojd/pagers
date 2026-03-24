use std::sync::atomic::Ordering;

use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::macros::horizontal;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use pagers_core::ops::Stats;
use pagers_core::output::pretty_size;

pub(crate) const SUMMARY_LINES: u16 = 5;

const LABEL_WIDTH: u16 = 17;

pub(crate) fn render_summary(
    stats: &Stats,
    elapsed: f64,
    label: &str,
    action_sign: isize,
    area: Rect,
    buf: &mut Buffer,
) {
    let page_size = *pagers_core::pagesize::PAGE_SIZE;
    let total_pages = stats.total_pages.load(Ordering::Relaxed);
    let initial = stats.initial_pages_in_core.load(Ordering::Relaxed);
    let action_pages = stats.action_pages.load(Ordering::Relaxed);
    let total_files = stats.total_files.load(Ordering::Relaxed);
    let total_dirs = stats.total_dirs.load(Ordering::Relaxed);

    let total_size = total_pages * page_size;
    let action_size = action_pages * page_size;
    let signed_action = (action_pages as isize) * action_sign;
    let resident_pages = initial.saturating_add_signed(signed_action);
    let resident_size = resident_pages * page_size;

    let label_style = Style::default().fg(Color::DarkGray);

    let mut cap = label.to_string();
    if let Some(c) = cap.get_mut(0..1) {
        c.make_ascii_uppercase();
    }

    let has_action = action_sign != 0;
    let mut rows: Vec<(String, Line)> = vec![
        ("Files:".into(), Line::from(format!("{total_files}"))),
        ("Directories:".into(), Line::from(format!("{total_dirs}"))),
    ];

    if has_action {
        rows.push((
            format!("{cap} Pages:"),
            pct_line(action_pages, total_pages, action_size, total_size),
        ));
    }

    rows.push((
        "Resident Pages:".into(),
        pct_line(resident_pages, total_pages, resident_size, total_size),
    ));

    rows.push((
        "Elapsed:".into(),
        Line::from(format!("{elapsed:.5} seconds")),
    ));

    let constraints: Vec<Constraint> = rows.iter().map(|_| Constraint::Length(1)).collect();
    let line_areas = Layout::vertical(constraints).split(area);
    for (i, (label_text, value_line)) in rows.into_iter().enumerate() {
        let [label_area, _, value_area] = horizontal![==LABEL_WIDTH, ==1, *=1].areas(line_areas[i]);

        Line::from(Span::styled(label_text, label_style))
            .alignment(Alignment::Right)
            .render(label_area, buf);

        value_line.render(value_area, buf);
    }
}

fn pct_line(pages: usize, total_pages: usize, size: usize, total_size: usize) -> Line<'static> {
    let pct = if total_pages > 0 {
        100.0 * pages as f64 / total_pages as f64
    } else {
        0.0
    };
    let mut spans = vec![Span::raw(format!(
        "{pages}/{total_pages}  {}/{}",
        pretty_size(size),
        pretty_size(total_size)
    ))];
    if total_pages > 0 {
        spans.push(Span::raw(format!("  {pct:.3}%")));
    }
    Line::from(spans)
}
