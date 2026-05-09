//! BRAINROT panel — chart only. Range chips at top, chart filling the rest,
//! a single uniform 1-row gap between them. Custom x-axis label painting
//! gives true proportional spacing (ratatui's built-in axis labels skew the
//! first and last gaps).

use crate::brainrot::aggregate::{bot_score, driver_is_bootstrapping, driver_score, Aggregate};
use crate::ledger_read::Record;
use crate::tui::style;
use crate::tui::{App, RangePreset};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::symbols;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Axis, Chart, Dataset, GraphType, Paragraph};
use ratatui::Frame;

/// Minimum records per bucket before bot_score is plotted. Heavily-shrunk
/// scores from tiny buckets cluster around 50 and just add visual noise
/// that hides the actual signal. This threshold matches the bot_score
/// confidence ramp's lower-end (10% confidence at n=5).
const MIN_BUCKET_N: usize = 5;

/// Compute the effective time window for chart rendering. Uses the active
/// data span (first → last record) when records exist; otherwise falls back
/// to the calendar range. Matches the metrics-tile sparkline behavior so a
/// "today" view with 30min of activity in the last hour spreads its data
/// across the full chart width instead of clumping on the right edge.
fn effective_range(app: &App) -> (f64, f64) {
    match (app.agg.first_ts, app.agg.last_ts) {
        (Some(f), Some(l)) if l > f => (f, l),
        _ => (app.range.since, app.range.until),
    }
}

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let inner = style::panel(f, area, "brainrot");

    // Vertical layout — four sections, all uniform 1-row gaps:
    //
    //   chips    (1 row)
    //   gap      (1 row)
    //   chart    (remaining height — graph fills to bottom of inner)
    //   xlabels  (1 row at the very bottom — custom painted)
    //
    // The xlabels row sits BELOW the chart graph, so we don't pass labels
    // to ratatui Chart (which would otherwise auto-render them with skewed
    // first/last spacing). Custom paint below ensures uniform distribution.
    let header_h: u16 = 1;
    let gap_h: u16 = 1;
    let xlabels_h: u16 = 1;
    let chart_h = inner
        .height
        .saturating_sub(header_h + gap_h + xlabels_h);
    let split = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(header_h),
            Constraint::Length(gap_h),
            Constraint::Length(chart_h),
            Constraint::Length(xlabels_h),
        ])
        .split(inner);

    let chart_area = pad_horizontal(split[2], 3, 2);
    let xlabels_area = pad_horizontal(split[3], 3, 2);

    range_chips(f, split[0], app);
    chart(f, chart_area, app);
    paint_x_labels(f, chart_area, xlabels_area, app);
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
        (RangePreset::ThisWeek, "wk"),
        (RangePreset::All, "all"),
    ];

    if area.height < 1 {
        return;
    }

    let chip_strip_width: u16 = presets
        .iter()
        .map(|(_, s)| s.chars().count() as u16 + 1)
        .sum::<u16>()
        + 1;

    // Single row: subtitle on the left, chip strip on the right. The
    // active chip is just CYAN text — no outline, no rectangle.
    let row = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(0), Constraint::Length(chip_strip_width)])
        .split(area);

    let subtitle = Line::from(vec![
        Span::raw(" "),
        Span::styled(
            format!("vibes over time · {}", app.range.label),
            Style::default().fg(style::CYAN),
        ),
    ]);
    f.render_widget(Paragraph::new(subtitle), row[0]);

    let mut spans: Vec<Span> = Vec::new();
    for (preset, short) in presets {
        let active = app.range_preset == *preset;
        let text_style = if active {
            Style::default().fg(style::CYAN)
        } else {
            style::dim()
        };
        spans.push(Span::styled(short.to_string(), text_style));
        spans.push(Span::raw(" "));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), row[1]);
}

fn chart(f: &mut Frame, area: Rect, app: &App) {
    let bucket_count = ((area.width as usize).saturating_sub(8)).max(20);
    let (since, until) = effective_range(app);
    let span = (until - since).max(1.0);
    let bucket_s = span / bucket_count as f64;

    let mut by_bucket: Vec<Vec<Record>> = (0..bucket_count).map(|_| Vec::new()).collect();
    for r in &app.agg.records {
        let idx = ((r.ts - since) / bucket_s).floor() as i64;
        let clamped = if idx < 0 { 0 } else { (idx as usize).min(bucket_count - 1) };
        by_bucket[clamped].push(r.clone());
    }

    // Driver bootstrap detection — when the baseline has no u_ch records yet,
    // every per-bucket driver_score returns 50 by design. Drawing that as a
    // flat horizontal line at the chart midline is misleading AND it sits
    // exactly where bot_score values cluster after sample-size shrinkage,
    // hiding the bot line entirely. Skip the driver series in that case.
    let driver_bootstrapping = driver_is_bootstrapping(&app.baseline);

    let mut bot_pts: Vec<(f64, f64)> = Vec::new();
    let mut drv_pts: Vec<(f64, f64)> = Vec::new();
    for (i, bucket) in by_bucket.into_iter().enumerate() {
        if bucket.len() < MIN_BUCKET_N {
            continue;
        }
        let mid_ts = since + bucket_s * (i as f64 + 0.5);
        let bucket_agg = Aggregate::ingest(bucket);
        bot_pts.push((mid_ts, bot_score(&bucket_agg, &app.baseline) as f64));
        if !driver_bootstrapping {
            drv_pts.push((mid_ts, driver_score(&bucket_agg, &app.baseline) as f64));
        }
    }

    let mut datasets = vec![Dataset::default()
        .name("bot")
        .marker(symbols::Marker::Braille)
        .graph_type(GraphType::Line)
        .style(Style::default().fg(style::PINK))
        .data(&bot_pts)];
    if !driver_bootstrapping {
        datasets.push(
            Dataset::default()
                .name("driver")
                .marker(symbols::Marker::Braille)
                .graph_type(GraphType::Line)
                .style(Style::default().fg(style::CYAN))
                .data(&drv_pts),
        );
    }

    // X-axis bounds match the effective range so the line spans the full
    // chart width — no x-axis labels passed (we paint our own below).
    let x_axis = Axis::default()
        .bounds([since, until])
        .style(Style::default().fg(style::GREY));

    let y_axis = Axis::default()
        .bounds([0.0, 100.0])
        // Two labels keeps the gutter to 2 cols (widest = "50") instead of
        // 3 ("100"). Top of chart implicitly = 100; user knows bounds are
        // 0..100 from context. Two intervals across the chart height also
        // means ratatui's per-row rounding stays uniform on any height.
        .labels(vec![
            Span::styled(" 0", style::dim()),
            Span::styled("50", style::dim()),
        ])
        .style(Style::default().fg(style::GREY));

    f.render_widget(
        Chart::new(datasets)
            .style(Style::default().bg(style::BG))
            .x_axis(x_axis)
            .y_axis(y_axis)
            .legend_position(None),
        area,
    );
}

/// Paint x-axis labels with TRUE uniform spacing along the actual plot
/// area. The plot area is `chart_area` minus the y-axis label/axis-line
/// padding on the left (4 cols for "100" + axis line). Labels are
/// positioned proportionally: first left-aligned, last right-aligned,
/// middles centered.
fn paint_x_labels(f: &mut Frame, chart_area: Rect, label_row: Rect, app: &App) {
    if label_row.height < 1 || chart_area.width < 8 {
        return;
    }

    // Match chart()'s x-axis bounds. Use active span when there's data so
    // labels reflect the actual time range the line covers, not the empty
    // calendar window.
    let (since, until) = effective_range(app);
    let span_secs = (until - since).max(1.0);
    // 7 labels for the 7-day range (one per day boundary), 12 otherwise.
    let n_labels: usize = if span_secs > 6.5 * 86400.0 && span_secs < 7.5 * 86400.0 {
        7
    } else {
        12
    };

    // Plot area: ratatui Chart reserves cols on the left for y-axis labels
    // (max width = "50" = 2 chars) plus 1 col for the axis line itself.
    // The plot area starts at chart_area.x + 3 and extends to the right edge.
    const Y_AXIS_PAD: u16 = 3;
    let plot_left = chart_area.x + Y_AXIS_PAD;
    let plot_right = chart_area.x + chart_area.width.saturating_sub(1);
    if plot_right <= plot_left {
        return;
    }
    let plot_width = (plot_right - plot_left + 1) as f64;

    let style = Style::default().fg(style::GREY);
    let buf_right = label_row.x + label_row.width;
    let buf = f.buffer_mut();

    for i in 0..n_labels {
        let fraction = if n_labels <= 1 {
            0.0
        } else {
            i as f64 / (n_labels - 1) as f64
        };
        let epoch = since + fraction * (until - since);
        let text = axis_label(epoch, span_secs);
        let text_w = text.chars().count() as u16;
        if text_w == 0 {
            continue;
        }

        // Target tick position in plot coords.
        let tick_x = plot_left + (fraction * (plot_width - 1.0)).round() as u16;

        // Alignment: first left-aligned, last right-aligned, middles centered.
        let label_x = if i == 0 {
            tick_x
        } else if i == n_labels - 1 {
            tick_x.saturating_sub(text_w.saturating_sub(1))
        } else {
            tick_x.saturating_sub(text_w / 2)
        };

        // Skip if start would be outside the label row's writable area.
        if label_x < label_row.x || label_x >= buf_right {
            continue;
        }

        // Paint each glyph; clip at right edge.
        for (offset, ch) in text.chars().enumerate() {
            let cx = label_x + offset as u16;
            if cx >= buf_right {
                break;
            }
            if let Some(cell) = buf.cell_mut((cx, label_row.y)) {
                cell.set_char(ch);
                cell.set_style(style);
            }
        }
    }
}

fn axis_label(epoch: f64, span_secs: f64) -> String {
    let secs = epoch as i64;
    let dt = time::OffsetDateTime::from_unix_timestamp(secs)
        .unwrap_or(time::OffsetDateTime::UNIX_EPOCH)
        .to_offset(crate::tui::style::local_offset());
    if span_secs < 4.0 * 3600.0 {
        format!("{:02}:{:02}", dt.hour(), dt.minute())
    } else if span_secs < 36.0 * 3600.0 {
        let h = if dt.minute() >= 30 {
            (dt.hour() + 1) % 24
        } else {
            dt.hour()
        };
        format!("{:02}:00", h)
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
