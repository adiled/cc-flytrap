//! BRAINROT panel — chart only. Range chips at top, chart suspended in
//! darkness with generous padding. The metrics strip (BOT/DRIVER/P99/CACHE)
//! used to live here; it now lives in panels::metrics so the brainrot
//! panel can read as a wide, broadcast-feeling chart instead of a
//! chart-plus-tile dashboard card.

use crate::brainrot::aggregate::{bot_score, driver_score, Aggregate};
use crate::ledger_read::Record;
use crate::tui::style;
use crate::tui::{App, RangePreset};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::symbols;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Axis, Chart, Dataset, GraphType, Paragraph};
use ratatui::Frame;

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let inner = style::panel(f, area, "brainrot");

    // Internal split: 16% header (subtitle + range chips), 68% chart,
    // 16% xaxis labels (the chart owns its own xaxis row, but the bottom
    // padding in this split keeps the chart from touching the panel border).
    // Combined with the LEFT/RIGHT padding below, the graph feels suspended
    // in darkness rather than wedged into the chrome.
    let h = inner.height as i32;
    // Use floor + remainder to stay integer-clean for tiny heights.
    let header_h = ((h as f32) * 0.16).round() as u16;
    let bottom_h = ((h as f32) * 0.16).round() as u16;
    let chart_h = inner.height.saturating_sub(header_h + bottom_h);
    let split = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(header_h.max(2)),
            Constraint::Length(chart_h),
            Constraint::Length(bottom_h),
        ])
        .split(inner);

    // Horizontal padding for the chart: left 3 cols, right 2 cols. Suspends
    // the plot away from the panel chrome so the borders breathe.
    let chart_area = pad_horizontal(split[1], 3, 2);

    range_chips(f, split[0], app);
    chart(f, chart_area, app);
}

fn pad_horizontal(area: Rect, left: u16, right: u16) -> Rect {
    let pad = left + right;
    if area.width <= pad {
        return area;
    }
    Rect {
        x: area.x + left,
        y: area.y,
        width: area.width - pad,
        height: area.height,
    }
}

fn range_chips(f: &mut Frame, area: Rect, app: &App) {
    let presets: &[(RangePreset, &str)] = &[
        (RangePreset::Today, "today"),
        (RangePreset::Yesterday, "yday"),
        (RangePreset::H24, "24h"),
        (RangePreset::Week, "7d"),
        (RangePreset::All, "all"),
    ];

    let chip_strip_width: u16 = presets
        .iter()
        .map(|(_, s)| s.chars().count() as u16 + 3)
        .sum::<u16>()
        + 1;

    // Header area is now only ~2 rows tall at most (16% of the chart panel).
    // We render the subtitle + chip text on the LAST visible row so the
    // active-chip outline (3 rows tall) fits when there's headroom.
    if area.height < 1 {
        return;
    }
    // The mid_row is the last row of the header area. The active chip
    // outline draws into the row above and below, so when area.height < 3
    // the outline simply truncates — the text still renders.
    let mid_y = area.y + area.height.saturating_sub(1);
    let mid_row = Rect {
        x: area.x,
        y: mid_y,
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
        let label = format!("  {}  ", short);
        let w = label.chars().count() as u16;
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

    // Active chip — solid cyan rectangle. Only fits when the header
    // section has at least 3 rows of headroom.
    if area.height >= 3 {
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
}

fn chart(f: &mut Frame, area: Rect, app: &App) {
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

fn axis_label(epoch: f64, span_secs: f64) -> String {
    let secs = epoch as i64;
    let dt = time::OffsetDateTime::from_unix_timestamp(secs)
        .unwrap_or(time::OffsetDateTime::UNIX_EPOCH)
        .to_offset(crate::tui::style::local_offset());
    if span_secs < 36.0 * 3600.0 {
        format!("{:02}:{:02}", dt.hour(), dt.minute())
    } else if span_secs < 30.0 * 86400.0 {
        format!("{}/{:02}", u8::from(dt.month()), dt.day())
    } else {
        format!(
            "{}-{:02}-{:02}",
            dt.year(),
            u8::from(dt.month()),
            dt.day()
        )
    }
}
