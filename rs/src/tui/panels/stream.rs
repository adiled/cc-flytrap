//! STREAM — compact recent-flow tail.

use crate::brainrot::aggregate::short_model;
use crate::tui::style;
use crate::tui::App;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let inner = style::panel(f, area, "stream");
    let local = crate::tui::style::local_offset();

    let mut recs = app.agg.records.clone();
    recs.sort_by(|a, b| b.ts.partial_cmp(&a.ts).unwrap_or(std::cmp::Ordering::Equal));

    let lines: Vec<Line> = recs
        .iter()
        .take(inner.height.saturating_sub(1) as usize)
        .map(|r| {
            let dt = time::OffsetDateTime::from_unix_timestamp(r.ts as i64)
                .unwrap_or(time::OffsetDateTime::UNIX_EPOCH)
                .to_offset(local);
            let when = format!("{:02}:{:02}:{:02}", dt.hour(), dt.minute(), dt.second());
            let model = short_model(r.model.as_deref().unwrap_or("?"));
            Line::from(vec![
                Span::styled(when, style::label()),
                Span::raw(" "),
                Span::styled(format!("in:{}", short_n(r.r#in)), style::value()),
                Span::raw(" "),
                Span::styled(format!("out:{}", short_n(r.out)), style::value()),
                Span::raw(" "),
                Span::styled(
                    format!("{}ms", r.lat),
                    Style::default().fg(style::heat_color(r.lat as f64)),
                ),
                Span::raw(" "),
                Span::styled(model, style::dim()),
            ])
        })
        .collect();

    f.render_widget(Paragraph::new(lines), inner);
}

fn short_n(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}k", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}
