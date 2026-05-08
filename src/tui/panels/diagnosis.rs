//! DIAGNOSIS — vibe label + diagnosis() + peaks/models summary.
//!
//! Text deliberately clustered in the upper-left of the panel; the rest
//! is intentional negative space. The brief: emptiness is the point —
//! diagnostics dominates by what it doesn't fill, not by what it shows.

use crate::brainrot::aggregate::{bot_score, diagnosis, driver_score, short_model, vibe_label};
use crate::tui::style;
use crate::tui::App;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let inner = style::panel(f, area, "diagnosis");

    let bot = bot_score(&app.agg);
    let drv = driver_score(&app.agg);
    let mut lines: Vec<Line> = Vec::new();

    lines.push(Line::from(vec![
        Span::styled("vibe:   ", style::dim()),
        Span::styled(
            format!("bot {}", vibe_label(bot)),
            Style::default().fg(style::score_color(bot)),
        ),
        Span::styled("  ·  ", style::dim()),
        Span::styled(
            format!("driver {}", vibe_label(drv)),
            Style::default().fg(style::score_color(drv)),
        ),
    ]));

    if let Some(d) = diagnosis(bot, drv) {
        lines.push(Line::from(vec![
            Span::styled("note:   ", style::dim()),
            Span::styled(d.to_string(), style::label()),
        ]));
    }

    if !app.agg.by_hour.is_empty() {
        let peak = app
            .agg
            .by_hour
            .iter()
            .max_by_key(|(_, b)| b.n)
            .map(|(h, b)| (*h, b.n))
            .unwrap_or((0, 0));
        let slow = app
            .agg
            .by_hour
            .iter()
            .max_by(|x, y| {
                let lx = x.1.lat_sum as f64 / x.1.n.max(1) as f64;
                let ly = y.1.lat_sum as f64 / y.1.n.max(1) as f64;
                lx.partial_cmp(&ly).unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(h, b)| (*h, b.lat_sum as f64 / b.n.max(1) as f64))
            .unwrap_or((0, 0.0));
        lines.push(Line::from(vec![
            Span::styled("peaks:  ", style::dim()),
            Span::styled(format!("busy {:02}:00 ({} reqs)", peak.0, peak.1), style::label()),
            Span::styled("  slow ", style::dim()),
            Span::styled(format!("{:02}:00", slow.0), style::label()),
            Span::styled(" (", style::dim()),
            Span::styled(format!("{:.0}ms", slow.1), Style::default().fg(style::heat_color(slow.1))),
            Span::styled(")", style::dim()),
        ]));
    }

    if !app.agg.models.is_empty() {
        let total: u64 = app.agg.models.values().sum();
        let mut sorted: Vec<(&String, &u64)> = app.agg.models.iter().collect();
        sorted.sort_by(|a, b| b.1.cmp(a.1));
        let bits: Vec<String> = sorted
            .iter()
            .take(3)
            .map(|(m, c)| {
                let pct = if total > 0 {
                    **c as f64 / total as f64 * 100.0
                } else {
                    0.0
                };
                format!("{} {:.0}%", short_model(m), pct)
            })
            .collect();
        lines.push(Line::from(vec![
            Span::styled("models: ", style::dim()),
            Span::styled(bits.join("  ·  "), style::label()),
        ]));
    }

    // Cluster text into the upper-left ~half of the panel. The Paragraph
    // doesn't wrap, so lines keep their natural width, but we constrain
    // the render rect to enforce that the lower-right is left empty even
    // when content could fit. The emptiness is the point.
    let line_count = lines.len() as u16;
    let text_h = (line_count + 1).min(inner.height);
    let text_w = (inner.width.saturating_mul(3) / 5).max(40).min(inner.width);
    let text_area = Rect {
        x: inner.x,
        y: inner.y,
        width: text_w,
        height: text_h,
    };

    f.render_widget(Paragraph::new(lines), text_area);
}
