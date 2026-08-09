use ratatui_core::buffer::Buffer;
use ratatui_core::layout::{Alignment, Constraint, Layout, Rect};
use ratatui_core::style::{Color, Style};
use ratatui_core::text::{Line, Span};
use ratatui_core::widgets::Widget;

use pagers_core::ops::Stats;
use pagers_core::output::{Summary, pretty_elapsed, pretty_size};

pub(crate) const SUMMARY_LINES: u16 = 5;

const LABEL_WIDTH: u16 = 17;

pub(crate) struct SummaryWidget<'a> {
    pub stats: &'a Stats,
    pub elapsed: f64,
    pub label: &'a str,
    pub action_sign: isize,
}

impl Widget for SummaryWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let summary = Summary::from_stats(self.stats, self.elapsed, self.action_sign);

        let label_style = Style::default().fg(Color::DarkGray);

        let mut cap = self.label.to_string();
        if let Some(c) = cap.get_mut(0..1) {
            c.make_ascii_uppercase();
        }

        let has_action = self.action_sign != 0;
        let mut rows: Vec<(String, Line)> = vec![
            ("Files:".into(), Line::from(summary.total_files.to_string())),
            (
                "Directories:".into(),
                Line::from(summary.total_dirs.to_string()),
            ),
        ];

        if has_action {
            rows.push((
                format!("{cap} Pages:"),
                pct_line(
                    summary.action_pages,
                    summary.total_pages,
                    summary.action_size,
                    summary.total_size,
                ),
            ));
        }

        rows.push((
            "Resident Pages:".into(),
            pct_line(
                summary.total_resident_pages,
                summary.total_pages,
                summary.resident_size,
                summary.total_size,
            ),
        ));

        rows.push(("Elapsed:".into(), Line::from(pretty_elapsed(self.elapsed))));

        let constraints: Vec<Constraint> = rows.iter().map(|_| Constraint::Length(1)).collect();
        let line_areas = Layout::vertical(constraints).split(area);
        for (i, (label_text, value_line)) in rows.into_iter().enumerate() {
            let [label_area, _, value_area] = Layout::horizontal([
                Constraint::Length(LABEL_WIDTH),
                Constraint::Length(1),
                Constraint::Fill(1),
            ])
            .areas(line_areas[i]);

            Line::from(Span::styled(label_text, label_style))
                .alignment(Alignment::Right)
                .render(label_area, buf);

            value_line.render(value_area, buf);
        }
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
