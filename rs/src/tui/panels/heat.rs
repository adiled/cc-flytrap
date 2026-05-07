//! HEAT — 24-hour activity bar.

use crate::tui::style;
use crate::tui::App;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

const SPARK: &[char] = &['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let inner = style::panel(f, area, "heat");

    let max_n: u64 = app.agg.by_hour.values().map(|b| b.n).max().unwrap_or(1);
    let mut spans: Vec<Span> = Vec::with_capacity(24);
    for h in 0u8..24 {
        if let Some(hb) = app.agg.by_hour.get(&h) {
            if hb.n == 0 {
                spans.push(Span::styled("·", style::dim()));
            } else {
                let intensity = ((hb.n as f64 / max_n as f64) * 7.0).min(7.0) as usize;
                let avg_lat = hb.lat_sum as f64 / hb.n as f64;
                spans.push(Span::styled(
                    SPARK[intensity].to_string(),
                    Style::default().fg(style::heat_color(avg_lat)),
                ));
            }
        } else {
            spans.push(Span::styled("·", style::dim()));
        }
    }

    let scale = Line::from(vec![Span::styled(
        "00      06      12      18  23",
        style::dim(),
    )]);
    let bar = Line::from(spans);

    f.render_widget(Paragraph::new(vec![scale, bar]), inner);
}
