//! ccft cyberpunk theme. Shared chrome, palette, and small render helpers
//! used by brainrot/perf/status. Plain ANSI — no ratatui dep — because every
//! ccft subcommand is print-and-exit, not a redraw loop.
//!
//! Branding rule: every section is preceded by a `header()` line that reads
//!   "▍ CCFT ▸ <subcommand> · <range>           v<version>"
//! followed by a half-block scanline rule. Colors stick to a small palette
//! (cyan primary, magenta accent, yellow/red for signal severity).

use std::sync::OnceLock;

/// True when stdout is not a TTY or NO_COLOR is set. Disables ANSI codes.
pub fn no_color() -> bool {
    static V: OnceLock<bool> = OnceLock::new();
    *V.get_or_init(|| {
        std::env::var_os("NO_COLOR").is_some()
            || !std::io::IsTerminal::is_terminal(&std::io::stdout())
    })
}

// ─── ANSI primitives ──────────────────────────────────────────────────────────
//
// We use 256-color codes for the core neon palette so the look is consistent
// regardless of terminal theme. 16-color fall-throughs (red/yellow/green) are
// kept for severity where the terminal's theme-tinted color is fine.

#[inline]
fn esc(code: &str, s: &str) -> String {
    if no_color() {
        s.to_string()
    } else {
        format!("\x1b[{}m{}\x1b[0m", code, s)
    }
}

pub fn dim(s: &str) -> String { esc("2", s) }
pub fn bold(s: &str) -> String { esc("1", s) }

// 256-color palette (the cyberpunk look)
const NEON_CYAN: &str = "38;5;51";       // cyan-100
const NEON_MAGENTA: &str = "38;5;201";   // hot pink
const NEON_GREEN: &str = "38;5;46";      // matrix green
const ACID_YELLOW: &str = "38;5;226";    // warning
const ALERT_RED: &str = "38;5;196";      // alert
const GHOST: &str = "38;5;240";          // dim grey separators
const SUBTLE: &str = "38;5;245";         // softer body text

pub fn cyan(s: &str) -> String { esc(NEON_CYAN, s) }
pub fn magenta(s: &str) -> String { esc(NEON_MAGENTA, s) }
pub fn green(s: &str) -> String { esc(NEON_GREEN, s) }
pub fn yellow(s: &str) -> String { esc(ACID_YELLOW, s) }
pub fn red(s: &str) -> String { esc(ALERT_RED, s) }
pub fn grey(s: &str) -> String { esc(GHOST, s) }
pub fn subtle(s: &str) -> String { esc(SUBTLE, s) }

/// Inverse-video on the brand color — used for the CCFT marker.
#[allow(dead_code)]
pub fn brand(s: &str) -> String {
    if no_color() {
        s.to_string()
    } else {
        format!("\x1b[7;{}m{}\x1b[0m", NEON_CYAN, s)
    }
}

// ─── Layout chrome ────────────────────────────────────────────────────────────

const BAR: char = '▍';      // left edge marker (1/4 block)
const SCAN: &str = "▔";     // upper half-block — used as a scanline rule

/// Standard ccft header. Always renders:
///
///     ▍ CCFT ▸ <subcommand> · <subtitle>
///     ▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔
///
/// `subtitle` may be empty.
pub fn header(subcommand: &str, subtitle: &str) {
    let bar = cyan(&BAR.to_string());
    let label = bold(&format!("CCFT"));
    let arrow = magenta("▸");
    let sub = bold(subcommand);
    let mid = if subtitle.is_empty() {
        String::new()
    } else {
        format!(" {} {}", grey("·"), subtle(subtitle))
    };
    println!();
    println!("  {} {} {} {}{}", bar, label, arrow, sub, mid);
    println!("  {}", grey(&SCAN.repeat(56)));
}

/// Section header: a magenta caret + bold label.
///
///     ▶ <name>
pub fn section(name: &str) {
    println!();
    println!("  {} {}", magenta("▶"), bold(name));
}

/// Soft rule between sub-blocks.
#[allow(dead_code)]
pub fn rule(width: usize) {
    println!("  {}", grey(&"─".repeat(width.min(72))));
}

/// Sub-bullet under a section line. Indented with the lead-in glyph.
pub fn bullet(s: &str) {
    println!("    {} {}", grey("↳"), s);
}

/// Footer / status line. Right-aligned by best-effort terminal width.
#[allow(dead_code)]
pub fn footer(label: &str) {
    println!();
    println!("  {} {}", grey("◇"), subtle(label));
}

// ─── Sparklines + bars ────────────────────────────────────────────────────────

const SPARK: &[char] = &['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

pub fn sparkline(values: &[f64], width: Option<usize>) -> String {
    if values.is_empty() {
        return String::new();
    }
    let series: Vec<f64> = match width {
        Some(w) if values.len() > w => {
            let bucket = values.len() as f64 / w as f64;
            (0..w)
                .map(|i| {
                    let lo = (i as f64 * bucket) as usize;
                    let hi = ((i as f64 + 1.0) * bucket) as usize;
                    let chunk = &values[lo..hi.min(values.len())];
                    if chunk.is_empty() {
                        0.0
                    } else {
                        chunk.iter().sum::<f64>() / chunk.len() as f64
                    }
                })
                .collect()
        }
        _ => values.to_vec(),
    };
    let mx = series.iter().cloned().fold(0.0_f64, f64::max).max(1.0);
    series
        .iter()
        .map(|&v| {
            let idx = ((v / mx) * 7.0).round() as usize;
            SPARK[idx.min(7)]
        })
        .collect()
}

/// `█████████░░░░░░░░` style filled bar.
pub fn bar(filled: usize, total: usize) -> String {
    let filled = filled.min(total);
    let empty = total - filled;
    format!("{}{}", "█".repeat(filled), "░".repeat(empty))
}

// ─── Heat coloring ────────────────────────────────────────────────────────────

/// Color a latency value (ms) according to severity thresholds.
pub fn heat_ms(latency_ms: f64, s: &str) -> String {
    if latency_ms < 500.0 {
        grey(s)
    } else if latency_ms < 1500.0 {
        green(s)
    } else if latency_ms < 3000.0 {
        cyan(s)
    } else if latency_ms < 6000.0 {
        yellow(s)
    } else {
        red(s)
    }
}

// ─── Number formatting ────────────────────────────────────────────────────────

pub fn fmt_n(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}k", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

pub fn fmt_dur(seconds: f64) -> String {
    if seconds < 60.0 {
        format!("{}s", seconds as u64)
    } else if seconds < 3600.0 {
        format!("{}m", (seconds / 60.0) as u64)
    } else if seconds < 86400.0 {
        let h = (seconds / 3600.0) as u64;
        let m = ((seconds % 3600.0) / 60.0) as u64;
        format!("{}h{}m", h, m)
    } else {
        format!("{}d", (seconds / 86400.0) as u64)
    }
}

pub fn fmt_ms(ms: f64) -> String {
    if ms < 1.0 {
        format!("{}us", (ms * 1000.0) as u64)
    } else if ms < 1000.0 {
        format!("{}ms", ms as u64)
    } else {
        format!("{:.1}s", ms / 1000.0)
    }
}

pub fn fmt_us(us: f64) -> String {
    if us < 1000.0 {
        format!("{}us", us as u64)
    } else if us < 1_000_000.0 {
        format!("{:.1}ms", us / 1000.0)
    } else {
        format!("{:.1}s", us / 1_000_000.0)
    }
}
