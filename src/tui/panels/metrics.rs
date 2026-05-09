//! METRICS strip — four hue-coded tiles, each its own bordered widget.
//!
//! Layout per tile (top to bottom):
//!
//!   title     — bold uppercase, in the tile's hue
//!   value     — the current aggregate, in the tile's hue
//!   sub-label — dim grey caption ("fine", "p99", "reuse")
//!   sparkline — bar histogram of the metric across the active range,
//!               in the tile's hue
//!
//! Tiles separated by 1-cell horizontal gutter (matching the rest of the
//! layout). Each tile gets its own corner-bracket panel chrome from the
//! shared `style::panel` renderer, so per-tile borders pick up the same
//! energized rail treatment as the larger panels.

use crate::brainrot::aggregate::{bot_score, driver_score, vibe_label, Aggregate};
use crate::ledger_read::{percentile, Record};
use crate::tui::style;
use crate::tui::App;
use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let cells = Layout::default()
        .direction(Direction::Horizontal)
        .spacing(1)
        .constraints([
            Constraint::Percentage(25),
            Constraint::Percentage(25),
            Constraint::Percentage(25),
            Constraint::Percentage(25),
        ])
        .split(area);

    // Whole-range aggregates (the BIG number per tile).
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

    // Sparkline series — bucket the records once across the active range
    // and compute each metric per bucket. Bucket count tracks tile inner
    // width so the sparkline naturally fits.
    let tile_inner_w = cells[0].width.saturating_sub(2) as usize;
    let n_buckets = tile_inner_w.max(8);
    let series = compute_series(app, n_buckets);

    tile(
        f,
        cells[0],
        "BOT",
        &bot.to_string(),
        &vibe_label(bot).to_string(),
        &series.bot,
        style::PINK,
    );
    tile(
        f,
        cells[1],
        "DRIVER",
        &drv.to_string(),
        &vibe_label(drv).to_string(),
        &series.driver,
        style::CYAN,
    );
    tile(
        f,
        cells[2],
        "P99 LAT",
        &format!("{}ms", p99),
        "p99",
        &series.p99,
        style::GOLD,
    );
    tile(
        f,
        cells[3],
        "CACHE",
        &format!("{:.0}%", cache_pct),
        "reuse",
        &series.cache,
        style::LIME,
    );
}

// ─── Tile rendering ──────────────────────────────────────────────────────────

fn tile(
    f: &mut Frame,
    area: Rect,
    label: &str,
    value: &str,
    sub: &str,
    series: &[f32],
    hue: Color,
) {
    // Empty title — the tile renders its own bigger label below the chrome.
    let inner = style::panel(f, area, "");
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    // Adaptive vertical layout. We always render the title; everything
    // else folds in as height permits.
    //
    //   height >= 4   →  title / value / sub-label / sparkline
    //   height == 3   →  title / value /             sparkline
    //   height == 2   →  title / value
    //   height == 1   →  title only
    let h = inner.height;
    let title_y = inner.y;
    let value_y = inner.y + 1;
    let sub_y = inner.y + 2;
    let spark_y = inner.y + h - 1;

    // Title — bold + hue, centered.
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            label.to_string(),
            Style::default().fg(hue).add_modifier(Modifier::BOLD),
        )))
        .alignment(Alignment::Center),
        Rect {
            x: inner.x,
            y: title_y,
            width: inner.width,
            height: 1,
        },
    );

    // Value — same hue, no bold (per the global "values aren't bold" rule).
    if h >= 2 {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                value.to_string(),
                Style::default().fg(hue),
            )))
            .alignment(Alignment::Center),
            Rect {
                x: inner.x,
                y: value_y,
                width: inner.width,
                height: 1,
            },
        );
    }

    // Sub-label — dim grey, centered. Only renders if it doesn't collide
    // with the sparkline row (need height >= 4).
    if h >= 4 {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                sub.to_string(),
                style::dim(),
            )))
            .alignment(Alignment::Center),
            Rect {
                x: inner.x,
                y: sub_y,
                width: inner.width,
                height: 1,
            },
        );
    }

    // Sparkline — paint into the bottom row directly. Skips when height
    // is too small to fit alongside title + value.
    if h >= 3 {
        paint_sparkline(f.buffer_mut(), inner.x, spark_y, inner.width, series, hue);
    }
}

// ─── Sparkline rendering ─────────────────────────────────────────────────────

fn paint_sparkline(
    buf: &mut Buffer,
    x: u16,
    y: u16,
    width: u16,
    series: &[f32],
    hue: Color,
) {
    const BARS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let max_w = width as usize;
    for (i, &v) in series.iter().take(max_w).enumerate() {
        let v = v.clamp(0.0, 1.0);
        if v < 0.05 {
            continue; // empty bucket — leave the cell as substrate
        }
        let idx = ((v * 7.99).floor() as usize).min(7);
        let cx = x + i as u16;
        if let Some(cell) = buf.cell_mut((cx, y)) {
            cell.set_char(BARS[idx]);
            cell.set_style(Style::default().fg(hue));
        }
    }
}

// ─── Per-bucket series computation ───────────────────────────────────────────

struct Series {
    bot: Vec<f32>,
    driver: Vec<f32>,
    p99: Vec<f32>,
    cache: Vec<f32>,
}

fn compute_series(app: &App, n: usize) -> Series {
    let span = (app.range.until - app.range.since).max(1.0);
    let bucket_s = span / n as f64;

    let mut buckets: Vec<Vec<Record>> = (0..n).map(|_| Vec::new()).collect();
    for r in &app.agg.records {
        let idx = ((r.ts - app.range.since) / bucket_s).floor() as i64;
        if idx >= 0 && (idx as usize) < n {
            buckets[idx as usize].push(r.clone());
        }
    }

    let mut bot = vec![0.0_f32; n];
    let mut driver = vec![0.0_f32; n];
    let mut p99 = vec![0.0_f32; n];
    let mut cache = vec![0.0_f32; n];

    for (i, bucket) in buckets.into_iter().enumerate() {
        if bucket.is_empty() {
            continue;
        }
        let mut lats: Vec<u64> = bucket.iter().map(|r| r.lat).collect();
        let p99_v = percentile(&mut lats, 99.0) as f32;
        let cr_total: u64 = bucket.iter().map(|r| r.cr).sum();
        let cc_total: u64 = bucket.iter().map(|r| r.cc).sum();

        let agg = Aggregate::ingest(bucket);
        bot[i] = bot_score(&agg) as f32 / 100.0;
        driver[i] = driver_score(&agg) as f32 / 100.0;
        p99[i] = p99_v;
        cache[i] = if cr_total + cc_total > 0 {
            cr_total as f32 / (cr_total + cc_total) as f32
        } else {
            0.0
        };
    }

    // Normalize p99 against the max in its own series — p99 latency has no
    // fixed ceiling like bot/driver/cache do, so the bars are relative.
    let p99_max = p99.iter().fold(0.0_f32, |a, b| a.max(*b)).max(1e-6);
    for v in p99.iter_mut() {
        *v /= p99_max;
    }

    Series {
        bot,
        driver,
        p99,
        cache,
    }
}
