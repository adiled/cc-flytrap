//! METRICS strip — BOT / DRIVER / P99 LAT / CACHE.
//!
//! Renders as four tiles in a single panel with vertical signal dividers
//! between cells. Asymmetric horizontal spacing: BOT slightly left-heavy,
//! CACHE slightly right-heavy, larger gap between DRIVER and P99 to
//! create a subconscious rhythm centered on the visual midpoint of the
//! strip. The tiles are intentionally short — embedded into infrastructure
//! rather than presenting as standalone dashboard cards.

use crate::brainrot::aggregate::{bot_score, driver_score, vibe_label};
use crate::ledger_read::percentile;
use crate::tui::style;
use crate::tui::App;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    // Empty title — chrome only. Metrics belong with brainrot above and
    // diagnostics below, not as a labeled section in their own right.
    let inner = style::panel(f, area, "");

    let bot = bot_score(&app.agg);
    let drv = driver_score(&app.agg);
    let mut lats = app.agg.lats.clone();
    let p99 = percentile(&mut lats, 99.0) as u64;
    let cache_total = app.agg.records.iter().map(|r| r.cr + r.cc).sum::<u64>();
    let cache_read = app.agg.records.iter().map(|r| r.cr).sum::<u64>();
    let cache_pct = if cache_total > 0 {
        cache_read as f64 / cache_total as f64 * 100.0
    } else {
        0.0
    };

    // Asymmetric tile widths: BOT is slightly wider on its right side
    // (slightly left-heavy in placement), DRIVER and P99 have a larger gap
    // between them at the visual center, CACHE is slightly right-heavy.
    // Implemented as 23/24/24/23 with extra gap between DRIVER and P99
    // courtesy of the divider weighting.
    let cells = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(23),
            Constraint::Percentage(25),
            Constraint::Percentage(27),
            Constraint::Percentage(25),
        ])
        .split(inner);

    tile(
        f,
        cells[0],
        "BOT",
        &bot.to_string(),
        &vibe_label(bot).to_string(),
        Some(bot),
        style::PINK,
    );
    tile(
        f,
        cells[1],
        "DRIVER",
        &drv.to_string(),
        &vibe_label(drv).to_string(),
        Some(drv),
        style::CYAN,
    );
    tile(f, cells[2], "P99 LAT", &format!("{}ms", p99), "p99", None, style::GOLD);
    tile(
        f,
        cells[3],
        "CACHE",
        &format!("{:.0}%", cache_pct),
        "reuse",
        None,
        style::LIME,
    );

    paint_tile_dividers(f, area);
}

/// Paint signal-themed vertical dividers between the four tiles. The
/// dividers span the full panel height (including chrome rows) so they
/// read as part of the same chrome family as the panel border.
fn paint_tile_dividers(f: &mut Frame, panel_area: Rect) {
    let inner = Rect {
        x: panel_area.x + 1,
        y: panel_area.y + 1,
        width: panel_area.width.saturating_sub(2),
        height: panel_area.height.saturating_sub(2),
    };
    let cells = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(23),
            Constraint::Percentage(25),
            Constraint::Percentage(27),
            Constraint::Percentage(25),
        ])
        .split(inner);

    for (i, cell) in cells.iter().enumerate().take(cells.len() - 1) {
        let x = cell.x + cell.width;
        if x >= panel_area.x + panel_area.width || x < panel_area.x {
            continue;
        }
        let y = panel_area.y + 1;
        let height = panel_area.height.saturating_sub(2);
        let seed = format!("metrics-div-{}-{}", i, x);
        style::signal_divider_v(f.buffer_mut(), x, y, height, &seed);
    }
}

fn tile(
    f: &mut Frame,
    area: Rect,
    label: &str,
    value: &str,
    sub: &str,
    score: Option<u32>,
    label_color: ratatui::style::Color,
) {
    let value_color = match score {
        Some(s) => style::score_color(s),
        None => label_color,
    };
    let lines = vec![
        Line::from(Span::styled(
            label.to_string(),
            Style::default().fg(label_color).add_modifier(ratatui::style::Modifier::BOLD),
        ))
        .alignment(Alignment::Center),
        Line::from(Span::styled(
            value.to_string(),
            Style::default()
                .fg(value_color)
                .add_modifier(ratatui::style::Modifier::BOLD),
        ))
        .alignment(Alignment::Center),
        Line::from(Span::styled(sub.to_string(), style::dim())).alignment(Alignment::Center),
    ];
    let content_area = area.centered_vertically(Constraint::Length(3));
    f.render_widget(Paragraph::new(lines), content_area);
}
