//! ccft perf — observability layer for ccft itself.
//!
//! "Is ccft slowing my requests down?" — decomposes per-request wall time into
//!   wall      total time the flow spent under ccft
//!   upstream  api.anthropic.com → us streaming duration  (= ledger lat)
//!   pre       wall - upstream                            (TTFB + our pre-work)
//!   ccft      measured internal processing time           (= ledger c_us)
//!
//! Verdict compares median ccft to median wall (apples-to-apples on records
//! that have c_us). Port of cc-flytrap/perf.py.

use crate::ledger_read::{iter_records, parse_range, percentile};
use crate::theme::*;

pub fn run(spec: &str) -> Result<(), Box<dyn std::error::Error>> {
    let range = parse_range(spec).map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;

    let mut walls: Vec<u64> = Vec::new();
    let mut upstreams: Vec<u64> = Vec::new();
    let mut pres: Vec<u64> = Vec::new();
    let mut ccfts: Vec<u64> = Vec::new();
    let mut walls_with_ccft: Vec<u64> = Vec::new();
    let mut n_total = 0;

    for r in iter_records(Some(range.since), Some(range.until)) {
        n_total += 1;
        let wall_ms = ((r.te - r.ts) * 1000.0).round() as i64;
        if wall_ms <= 0 {
            continue;
        }
        let wall_ms = wall_ms as u64;
        walls.push(wall_ms);
        upstreams.push(r.lat);
        pres.push(wall_ms.saturating_sub(r.lat));
        if let Some(c) = r.c_us {
            if c > 0 {
                ccfts.push(c);
                walls_with_ccft.push(wall_ms);
            }
        }
    }

    header("perf", &range.label);

    if n_total == 0 {
        bullet(&dim("(no records in range)"));
        println!();
        return Ok(());
    }

    show_row("wall", &mut walls, fmt_ms_f, None);
    show_row("upstream", &mut upstreams, fmt_ms_f, Some(&dim));
    show_row("pre", &mut pres, fmt_ms_f, Some(&dim));
    if !ccfts.is_empty() {
        show_row("ccft", &mut ccfts, fmt_us_f, Some(&cyan));
    } else {
        println!(
            "  {:10}  {}",
            bold("ccft"),
            dim("no records with ccft timing yet — run more traffic")
        );
    }

    let n_with_ccft = ccfts.len();
    let coverage_pct = if n_total > 0 {
        n_with_ccft as f64 / n_total as f64 * 100.0
    } else {
        0.0
    };

    section("records");
    println!(
        "    {} {} {} {} with ccft timing  {} wall = upstream + pre",
        bold(&n_total.to_string()),
        grey("·"),
        bold(&n_with_ccft.to_string()),
        grey(""),
        grey("·"),
    );

    if !ccfts.is_empty() {
        let mut walls_with_ccft_mut = walls_with_ccft.clone();
        let ccft_p50 = percentile(&mut ccfts.clone(), 50.0);
        let wall_p50 = percentile(&mut walls_with_ccft_mut, 50.0);
        let (kind, msg) = verdict(ccft_p50, wall_p50, coverage_pct);
        section("verdict");
        let glyph = match kind {
            "clean" => green("◆"),
            "small" => green("◆"),
            "measurable" => yellow("◇"),
            _ => red("⚠"),
        };
        println!("    {} {}", glyph, msg);
    }

    println!();
    Ok(())
}

fn show_row<F>(name: &str, values: &mut [u64], formatter: F, color: Option<&dyn Fn(&str) -> String>)
where
    F: Fn(f64) -> String,
{
    let mut a = values.to_vec();
    let mut b = values.to_vec();
    let mut c = values.to_vec();
    let p50 = percentile(&mut a, 50.0);
    let p95 = percentile(&mut b, 95.0);
    let p99 = percentile(&mut c, 99.0);
    let col = |s: &str| match color {
        Some(f) => f(s),
        None => s.to_string(),
    };
    println!(
        "  {:10}  p50 {:>10}  p95 {:>10}  p99 {:>10}",
        bold(name),
        col(&formatter(p50)),
        col(&formatter(p95)),
        col(&formatter(p99))
    );
}

fn fmt_ms_f(v: f64) -> String {
    fmt_ms(v)
}

fn fmt_us_f(v: f64) -> String {
    fmt_us(v)
}

fn verdict(ccft_p50_us: f64, wall_p50_ms: f64, coverage_pct: f64) -> (&'static str, String) {
    let ccft_ms = ccft_p50_us / 1000.0;
    let rel = if wall_p50_ms > 0.0 {
        ccft_ms / wall_p50_ms * 100.0
    } else {
        0.0
    };
    let warn = if coverage_pct < 5.0 {
        format!("  {}", dim(&format!("(small sample — {:.0}% of records)", coverage_pct)))
    } else {
        String::new()
    };

    if ccft_ms < 5.0 && rel < 1.0 {
        ("clean", format!("{}{}", green(&format!(
            "ccft contributes ~{:.2}% of wall time. not the bottleneck — slowness is upstream.",
            rel
        )), warn))
    } else if ccft_ms < 30.0 && rel < 3.0 {
        ("small", format!("{}{}", green(&format!(
            "ccft adds ~{:.1}ms median ({:.1}%). small, well within network noise.",
            ccft_ms, rel
        )), warn))
    } else if ccft_ms < 100.0 && rel < 10.0 {
        ("measurable", format!("{}{}", yellow(&format!(
            "ccft adds ~{:.0}ms median ({:.0}%). measurable but probably acceptable.",
            ccft_ms, rel
        )), warn))
    } else {
        ("investigate", format!("{}{}", red(&format!(
            "⚠ ccft adds ~{:.0}ms median ({:.0}% of wall). worth investigating.",
            ccft_ms, rel
        )), warn))
    }
}
