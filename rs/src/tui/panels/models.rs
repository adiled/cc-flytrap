//! MODELS panel — list with online dots.

use crate::brainrot::aggregate::short_model;
use crate::tui::style;
use crate::tui::App;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let inner = style::panel(f, area, "models");

    let total: u64 = app.agg.models.values().sum();
    let mut sorted: Vec<(&String, &u64)> = app.agg.models.iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(a.1));

    let mut lines: Vec<Line> = Vec::new();
    for (model, count) in sorted.iter().take(5) {
        let pct = if total > 0 {
            **count as f64 / total as f64 * 100.0
        } else {
            0.0
        };
        let dot = if **count > 0 {
            Span::styled("◉", Style::default().fg(style::LIME))
        } else {
            Span::styled("⊙", Style::default().fg(style::GREY))
        };
        let mut name = short_model(model);
        if name.len() > 11 {
            name.truncate(11);
        }
        lines.push(Line::from(vec![
            dot,
            Span::raw(" "),
            Span::styled(format!("{:11}", name), style::label()),
            Span::styled(format!("{:>3}%", pct as u32), style::value()),
        ]));
    }
    if lines.is_empty() {
        lines.push(Line::from(Span::styled("(no traffic)", style::dim())));
    }

    f.render_widget(Paragraph::new(lines), inner);
}
