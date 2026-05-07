//! LEDGER — newest-first table.

use crate::tui::style;
use crate::tui::App;
use ratatui::layout::{Constraint, Rect};
use ratatui::style::Style;
use ratatui::text::Span;
use ratatui::widgets::{Cell, Row, Table};
use ratatui::Frame;

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let inner = style::panel(f, area, "ledger");
    let local = crate::tui::style::local_offset();

    let header = Row::new(vec![
        Cell::from(Span::styled("TIME", style::dim())),
        Cell::from(Span::styled("Δin", style::dim())),
        Cell::from(Span::styled("Δout", style::dim())),
        Cell::from(Span::styled("lat", style::dim())),
    ]);

    let mut recs = app.agg.records.clone();
    recs.sort_by(|a, b| b.ts.partial_cmp(&a.ts).unwrap_or(std::cmp::Ordering::Equal));

    let rows: Vec<Row> = recs
        .iter()
        .take(inner.height.saturating_sub(2) as usize)
        .map(|r| {
            let dt = time::OffsetDateTime::from_unix_timestamp(r.ts as i64)
                .unwrap_or(time::OffsetDateTime::UNIX_EPOCH)
                .to_offset(local);
            let when = format!("{:02}:{:02}:{:02}", dt.hour(), dt.minute(), dt.second());
            let lat_style = Style::default().fg(style::heat_color(r.lat as f64));
            Row::new(vec![
                Cell::from(Span::styled(when, style::label())),
                Cell::from(Span::styled(format!("+{}", short_n(r.r#in)), style::value())),
                Cell::from(Span::styled(format!("+{}", short_n(r.out)), style::value())),
                Cell::from(Span::styled(format!("{}ms", r.lat), lat_style)),
            ])
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Length(8),
            Constraint::Length(7),
            Constraint::Length(7),
            Constraint::Length(7),
        ],
    )
    .header(header);
    f.render_widget(table, inner);
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
