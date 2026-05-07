//! BRAINROT panel — chart + tile row, no border, range chips with outlined
//! pink rect on the active preset.

use crate::brainrot::aggregate::{bot_score, driver_score, vibe_label, Aggregate};
use crate::ledger_read::{percentile, Record};
use crate::tui::style;
use crate::tui::{App, RangePreset};
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::symbols;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Axis, Chart, Dataset, GraphType, Paragraph};
use ratatui::Frame;

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let split = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(13), Constraint::Length(7)])
        .split(area);

    let chart_inner = style::panel(f, split[0], "brainrot");
    // 3-row chip area + chart. The 3 rows give selected chips room for a
    // real signal-bordered rectangle (top edge, text+sides, bottom edge).
    let chart_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // subtitle + chips (with selected outline)
            Constraint::Min(0),    // chart
        ])
        .split(chart_inner);
    range_chips(f, chart_layout[0], app);
    chart(f, chart_layout[1], app);

    let tile_inner = style::panel(f, split[1], "");
    tiles(f, tile_inner, app);
    // Tile dividers span the full TILE PANEL height (including chrome rows)
    // so they read as part of the same chrome family. Drawn on the parent
    // split[1] so they extend through the panel borders, not just the inner.
    paint_tile_dividers(f, split[1], app);
}

fn range_chips(f: &mut Frame, area: Rect, app: &App) {
    let presets: &[(RangePreset, &str)] = &[
        (RangePreset::Today, "today"),
        (RangePreset::Yesterday, "yday"),
        (RangePreset::H24, "24h"),
        (RangePreset::Week, "7d"),
        (RangePreset::All, "all"),
    ];

    // Compute the chip strip width (sum of " short " + space-separator).
    let chip_strip_width: u16 = presets
        .iter()
        .map(|(_, s)| s.chars().count() as u16 + 3)
        .sum::<u16>()
        + 1;

    // The 3-row chip area: top row reserved for outline top edges, middle
    // row holds subtitle + chip text, bottom row reserved for outline
    // bottom edges. Render content on the middle row only.
    let mid_row = Rect {
        x: area.x,
        y: area.y + 1,
        width: area.width,
        height: 1,
    };

    let row = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(0), Constraint::Length(chip_strip_width)])
        .split(mid_row);

    let subtitle = Line::from(vec![
        Span::raw(" "),
        Span::styled(
            format!("vibes over time · {}", app.range.label),
            Style::default().fg(style::CYAN),
        ),
    ]);
    f.render_widget(Paragraph::new(subtitle), row[0]);

    let mut spans: Vec<Span> = Vec::new();
    let mut chip_text_rects: Vec<(Rect, bool)> = Vec::new();
    let mut cursor_x = row[1].x;
    for (preset, short) in presets {
        let active = app.range_preset == *preset;
        // 2-cell horizontal padding ("  short  ") visually balances the
        // 1-row vertical padding because terminal cells are ~2× taller
        // than they are wide. Equal-feeling padding all around the chip.
        let label = format!("  {}  ", short);
        let w = label.chars().count() as u16;
        // Active chip text uses bright cyan; the solid cyan outline (drawn
        // below) provides the "selected" visual signal without competing
        // with the streaky panel chrome.
        let text_style = if active {
            Style::default().fg(style::CYAN)
        } else {
            style::dim()
        };
        spans.push(Span::styled(label, text_style));
        spans.push(Span::raw(" "));
        chip_text_rects.push((
            Rect {
                x: cursor_x,
                y: mid_row.y,
                width: w,
                height: 1,
            },
            active,
        ));
        cursor_x += w + 1;
    }
    f.render_widget(Paragraph::new(Line::from(spans)), row[1]);

    // Active chip → 3-row solid-cyan rounded rectangle. Different
    // thematics from the panel chrome (which uses streaky signal):
    // chips are buttons, not container chrome, so a clean uniform
    // outline reads as "selectable element" rather than blending in.
    for (text_rect, active) in chip_text_rects {
        if !active {
            continue;
        }
        let outline = Rect {
            x: text_rect.x,
            y: text_rect.y.saturating_sub(1),
            width: text_rect.width,
            height: 3,
        };
        style::solid_rect(f.buffer_mut(), outline, style::CYAN);
    }
}

// (helper removed: spans_width was only used by the old chip layout)

fn chart(f: &mut Frame, area: Rect, app: &App) {
    // True time-series: x-axis is epoch seconds across the active range.
    // Bucket the in-memory records by time, aggregate each bucket, plot the
    // resulting (bucket_midpoint_epoch, score) points. Empty buckets are
    // skipped so the line interpolates over gaps instead of dropping to 0.
    let bucket_count = ((area.width as usize).saturating_sub(8)).max(20);
    let span = (app.range.until - app.range.since).max(1.0);
    let bucket_s = span / bucket_count as f64;

    let mut by_bucket: Vec<Vec<Record>> = (0..bucket_count).map(|_| Vec::new()).collect();
    for r in &app.agg.records {
        let idx = ((r.ts - app.range.since) / bucket_s).floor() as i64;
        if idx >= 0 && (idx as usize) < bucket_count {
            by_bucket[idx as usize].push(r.clone());
        }
    }

    let mut bot_pts: Vec<(f64, f64)> = Vec::new();
    let mut drv_pts: Vec<(f64, f64)> = Vec::new();
    for (i, bucket) in by_bucket.into_iter().enumerate() {
        if bucket.is_empty() {
            continue;
        }
        let mid_ts = app.range.since + bucket_s * (i as f64 + 0.5);
        let bucket_agg = Aggregate::ingest(bucket);
        bot_pts.push((mid_ts, bot_score(&bucket_agg) as f64));
        drv_pts.push((mid_ts, driver_score(&bucket_agg) as f64));
    }

    let datasets = vec![
        Dataset::default()
            .name("bot")
            .marker(symbols::Marker::HalfBlock)
            .graph_type(GraphType::Line)
            .style(Style::default().fg(style::PINK))
            .data(&bot_pts),
        Dataset::default()
            .name("driver")
            .marker(symbols::Marker::HalfBlock)
            .graph_type(GraphType::Line)
            .style(Style::default().fg(style::CYAN))
            .data(&drv_pts),
    ];

    // Three evenly-spaced labels across the range. ratatui's Axis widget
    // has a documented bug at >3 labels where middle labels collapse —
    // 3 is the upstream-supported maximum for both axes. (See ratatui
    // chart.rs comment at the labels() docstring.)
    let n_labels = 3;
    let span_secs = (app.range.until - app.range.since).max(1.0);
    let x_labels: Vec<Span> = (0..n_labels)
        .map(|i| {
            let t = app.range.since
                + (app.range.until - app.range.since) * (i as f64 / (n_labels - 1) as f64);
            Span::styled(axis_label(t, span_secs), style::dim())
        })
        .collect();

    let x_axis = Axis::default()
        .bounds([app.range.since, app.range.until])
        .labels(x_labels)
        .style(Style::default().fg(style::GREY));

    let y_axis = Axis::default()
        .bounds([0.0, 100.0])
        .labels(vec![
            Span::styled("0", style::dim()),
            Span::styled("50", style::dim()),
            Span::styled("100", style::dim()),
        ])
        .style(Style::default().fg(style::GREY));

    f.render_widget(
        Chart::new(datasets)
            .style(Style::default().bg(style::BG))
            .x_axis(x_axis)
            .y_axis(y_axis),
        area,
    );
}

fn tiles(f: &mut Frame, area: Rect, app: &App) {
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

    let cells = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Ratio(1, 4),
            Constraint::Ratio(1, 4),
            Constraint::Ratio(1, 4),
            Constraint::Ratio(1, 4),
        ])
        .split(area);

    tile(f, cells[0], "BOT", &bot.to_string(), &vibe_label(bot).to_string(), Some(bot), style::PINK);
    tile(f, cells[1], "DRIVER", &drv.to_string(), &vibe_label(drv).to_string(), Some(drv), style::CYAN);
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
}

/// Paint signal-themed vertical dividers between the four tiles. The
/// dividers span the full TILE PANEL height (including the chrome rows)
/// so they read as part of the same chrome family as the panel border.
fn paint_tile_dividers(f: &mut Frame, panel_area: Rect, app: &App) {
    let _ = app;
    // Re-derive the inner-area split that `tiles()` used so we know where
    // the boundaries between tile cells live.
    let inner = Rect {
        x: panel_area.x + 1,
        y: panel_area.y + 1,
        width: panel_area.width.saturating_sub(2),
        height: panel_area.height.saturating_sub(2),
    };
    let cells = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Ratio(1, 4),
            Constraint::Ratio(1, 4),
            Constraint::Ratio(1, 4),
            Constraint::Ratio(1, 4),
        ])
        .split(inner);

    for (i, cell) in cells.iter().enumerate().take(cells.len() - 1) {
        let x = cell.x + cell.width;
        if x >= panel_area.x + panel_area.width || x < panel_area.x {
            continue;
        }
        // Span from one row below the panel's top edge to one row above
        // the bottom edge — same vertical extent as the visible chrome.
        let y = panel_area.y + 1;
        let height = panel_area.height.saturating_sub(2);
        let seed = format!("tile-div-{}-{}", i, x);
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
    // Vertically center the 3-line content block within the tile cell.
    // ratatui's Rect::centered_vertically does this responsively — when the
    // tile is short, the content shrinks to fit; when tall, it floats centered.
    let content_area = area.centered_vertically(Constraint::Length(3));
    f.render_widget(Paragraph::new(lines), content_area);
}

fn axis_label(epoch: f64, span_secs: f64) -> String {
    let secs = epoch as i64;
    let dt = time::OffsetDateTime::from_unix_timestamp(secs)
        .unwrap_or(time::OffsetDateTime::UNIX_EPOCH)
        .to_offset(crate::tui::style::local_offset());
    if span_secs < 36.0 * 3600.0 {
        // Sub-day → HH:MM
        format!("{:02}:{:02}", dt.hour(), dt.minute())
    } else if span_secs < 30.0 * 86400.0 {
        // Up to a month → M/D
        format!("{}/{:02}", u8::from(dt.month()), dt.day())
    } else {
        // Longer → ISO-ish date
        format!(
            "{}-{:02}-{:02}",
            dt.year(),
            u8::from(dt.month()),
            dt.day()
        )
    }
}
