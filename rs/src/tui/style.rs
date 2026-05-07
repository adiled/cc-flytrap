//! TUI palette + small chrome helpers (panel title pip, corner accents).
//! Outlines are replaced with accents — no Block borders anywhere.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use std::sync::OnceLock;

/// Cached local UTC offset. `time::UtcOffset::current_local_offset()` calls
/// non-thread-safe libc TZ functions and can SIGABRT on macOS in MT contexts.
/// We resolve it ONCE at startup (single-threaded) and reuse forever.
pub fn local_offset() -> time::UtcOffset {
    static V: OnceLock<time::UtcOffset> = OnceLock::new();
    *V.get_or_init(|| {
        time::UtcOffset::current_local_offset().unwrap_or(time::UtcOffset::UTC)
    })
}

// ─── Palette ──────────────────────────────────────────────────────────────────
//
// Five neon hues + grey + white on pure black. Each metric/category carries a
// single canonical hue used everywhere it appears (label, line, tile, tag).

pub const PINK: Color = Color::Rgb(0xff, 0x1f, 0x8c); // brand / titles / COPE
pub const CYAN: Color = Color::Rgb(0x00, 0xe5, 0xff); // body / DELUSION / values
pub const GOLD: Color = Color::Rgb(0xff, 0xd6, 0x00); // SCHIZO / warn
pub const LIME: Color = Color::Rgb(0x39, 0xff, 0x14); // SILLINESS / online / OK
pub const VIOLET: Color = Color::Rgb(0xa8, 0x55, 0xf7); // DOOMER / accent series
pub const WHITE: Color = Color::Rgb(0xe6, 0xe9, 0xee); // numeric / readouts
pub const GREY: Color = Color::Rgb(0x3f, 0x44, 0x52); // off / scaffold
pub const SUBTLE: Color = Color::Rgb(0x6b, 0x72, 0x80); // dim body, separators

// Border accent: a muted, hue-preserved pink used for panel chrome only.
// Distinct from the bright brand PINK so the chrome reads as a translucent
// seam rather than a brand statement.
pub const SEAM: Color = Color::Rgb(0x7a, 0x33, 0x5c); // ~PINK at 50% brightness

// Frame background — RGB(2, 5, 14) / #02050E. The exact substrate the frame
// is painted with; energy-fade math lerps toward this so dim cells dissolve
// cleanly into the ground instead of through a different shade of black.
pub const BG: Color = Color::Rgb(0x02, 0x05, 0x0e);

// Backward-compatibility aliases used by panels written before the repaint.
pub const MAGENTA: Color = PINK;
pub const GREEN: Color = LIME;
pub const YELLOW: Color = GOLD;
pub const RED: Color = PINK;
pub const GHOST: Color = GREY;

pub fn title() -> Style {
    Style::default().fg(PINK).add_modifier(Modifier::BOLD)
}

pub fn brand() -> Style {
    Style::default().fg(PINK).add_modifier(Modifier::BOLD)
}

pub fn label() -> Style {
    Style::default().fg(SUBTLE)
}

pub fn value() -> Style {
    Style::default().fg(WHITE).add_modifier(Modifier::BOLD)
}

pub fn dim() -> Style {
    Style::default().fg(GREY)
}

#[allow(dead_code)]
pub fn good() -> Style {
    Style::default().fg(LIME)
}

#[allow(dead_code)]
pub fn warn() -> Style {
    Style::default().fg(GOLD)
}

#[allow(dead_code)]
pub fn alert() -> Style {
    Style::default().fg(PINK)
}

pub fn key_hint() -> Style {
    Style::default().fg(PINK).add_modifier(Modifier::BOLD)
}

pub fn active_chip() -> Style {
    Style::default().fg(PINK).add_modifier(Modifier::BOLD)
}

pub fn score_color(score: u32) -> Color {
    if score < 40 {
        LIME
    } else if score < 70 {
        GOLD
    } else {
        PINK
    }
}

pub fn heat_color(latency_ms: f64) -> Color {
    if latency_ms < 500.0 {
        SUBTLE
    } else if latency_ms < 1500.0 {
        LIME
    } else if latency_ms < 3000.0 {
        CYAN
    } else if latency_ms < 6000.0 {
        GOLD
    } else {
        PINK
    }
}

// ─── Panel chrome: L-shaped corner accents with gradient fade to nothing ─────
//
// Each panel renders four L-brackets, one at each corner. Each L has a long
// horizontal arm and a short vertical arm. The first cell at the actual corner
// is brightest PINK; cells along the arm fade RGB-linearly to fully transparent
// (we just stop painting past the threshold, leaving terminal background).
// Mid-edge is empty space — the panels are delineated by partial corners +
// spatial separation, not continuous borders.
//
// Title pip lives at row 0, col 2: " // LABEL " with `// ` in dim white and
// LABEL in pink bold.

// Border = a low-resolution light field around each panel's perimeter.
//
// The model is intentionally simple: every cell rolls a weighted random
// luminance value. Most cells land in the mid range (0.32-0.55). A small
// fraction (~4%) become BURSTS (luminance 0.85-1.0, brightened in their
// own hue family — never washed out to white). Corners are always bursts.
// Some bursts noise-corrupt an adjacent cell to near-zero opacity; that
// gives the line its "imperfect display" character. A slow time-driven
// drift modulates burst selection so individual cells breathe in and out
// of overload over ~16 second cycles without animating geometry.
//
// HUE comes from a separate slow sine drift between polluted magenta and
// polluted cyan, so the perimeter passes through long magenta zones and
// long cyan zones with brief purple in the transitions.
//
// What this model NO LONGER has (deliberately): no momentum, no bleed
// kernels, no pressure curve, no hotspot peak-detection, no contamination
// pass, no phosphor flare pass, no named decorative effects. Past
// iterations layered "effects" on top of a smooth signal, which made the
// border read as "renderer narrating its mechanism." This is a single
// weighted distribution.

// Hue field — one parameter only.
const LAMBDA_HUE: f32 = 220.0;

// Luminance distribution weights.
const BURST_RATE: f32 = 0.04; // ~4% of cells are bursts
const NOISE_AT_BURST: f32 = 0.30; // 30% of bursts noise-corrupt an adjacent cell
const MID_LUM_MIN: f32 = 0.32;
const MID_LUM_MAX: f32 = 0.55;
const BURST_LUM_MIN: f32 = 0.85;
const BURST_LUM_MAX: f32 = 1.00;
const NOISE_LUM: f32 = 0.04;

// Polluted neon — never pure #ff00ff/#00ffff. Magenta is red-tinged, cyan is
// blue-tinged. Slightly desaturated so they don't scream.
const NEON_MAGENTA: Color = Color::Rgb(0xe2, 0x2a, 0x78);
const NEON_CYAN: Color = Color::Rgb(0x2c, 0xa6, 0xc4);

pub fn panel(f: &mut Frame, area: Rect, label: &str) -> Rect {
    if area.width < 4 || area.height < 3 {
        return area;
    }
    paint_corner_accents(f.buffer_mut(), area);

    // Title pip: " // LABEL " in row 0, starting at col 2. Empty label =
    // chrome only (used for sub-panels like the BRAINROT score-tile row).
    if !label.is_empty() {
        let pip_x = area.x.saturating_add(2);
        if pip_x < area.x + area.width {
            let prefix = "// ";
            let label_uc = label.to_uppercase();
            let pip_w = (prefix.chars().count() + label_uc.chars().count() + 2)
                .min((area.x + area.width).saturating_sub(pip_x) as usize) as u16;
            let title_rect = Rect {
                x: pip_x,
                y: area.y,
                width: pip_w,
                height: 1,
            };
            let line = Line::from(vec![
                Span::raw(" "),
                Span::styled(prefix, Style::default().fg(SUBTLE)),
                Span::styled(label_uc, title()),
                Span::raw(" "),
            ]);
            f.render_widget(Paragraph::new(line), title_rect);
        }
    }

    Rect {
        x: area.x + 1,
        y: area.y + 1,
        width: area.width.saturating_sub(2),
        height: area.height.saturating_sub(2),
    }
}

/// Energized perimeter ribbon. The panel border is rendered as a sampled
/// 1D signal that wraps clockwise around the rectangle — one continuous
/// loop, not four separate edges. Every cell paints; brightness and hue
/// come from smooth low-frequency analog fields plus a post-pass
/// exponential-moving-average smoother that gives the line *inertia*
/// (energy persists across nearby cells, so streaks don't atomize).
fn paint_corner_accents(buf: &mut Buffer, area: Rect) {
    let seed = panel_label_seed(buf.area, area);
    paint_panel_border(buf, area, &seed);
}

/// Generic energized-line painter for any 1D path of cells. Used by tile
/// dividers and other elements that want the perimeter rail look but NOT
/// the substrate halo (those would bleed into adjacent content).
pub fn paint_signal(buf: &mut Buffer, cells: &[(u16, u16, char)], seed: &str) {
    let n = cells.len();
    if n < 4 {
        return;
    }
    let t = time_offset();
    let hue = compute_hue_field(n, seed);
    let (luminance, _is_burst) = compute_perimeter_luminance(cells, seed, t);
    paint_cells_with_signal(buf, cells, &hue, &luminance, seed);
}

/// Panel border = perimeter signal + a single subliminal substrate halo.
/// All variation is one weighted-random luminance + a slow hue field.
fn paint_panel_border(buf: &mut Buffer, area: Rect, seed: &str) {
    if area.width < 4 || area.height < 3 {
        return;
    }
    let cells = collect_perimeter(area);
    let n = cells.len();
    if n < 4 {
        return;
    }
    let t = time_offset();
    let hue = compute_hue_field(n, seed);
    let (luminance, is_burst) = compute_perimeter_luminance(&cells, seed, t);

    paint_cells_with_signal(buf, &cells, &hue, &luminance, seed);
    paint_outward_glow(buf, area, &cells, &hue, &is_burst, seed);
}


// ─── Hue field ───────────────────────────────────────────────────────────────
//
// One slow-wavelength sine drift between polluted magenta (~0) and polluted
// cyan (~1). Long wavelength gives long magenta zones and long cyan zones
// with brief purple in the transitions. Cyclic EMA smooths the discrete
// per-cell sampling. NO temporal drift on hue — color zones stay stable
// across frames so the eye reads the panel's color identity instead of
// chasing motion.

fn compute_hue_field(n: usize, seed: &str) -> Vec<f32> {
    let p_hue = phase_from(seed, "hue");
    let mut hue: Vec<f32> = Vec::with_capacity(n);
    for i in 0..n {
        let p = i as f32;
        let h = (p * std::f32::consts::TAU / LAMBDA_HUE + p_hue).sin();
        let sharpened = (h * 1.3).tanh();
        hue.push((sharpened + 1.0) * 0.5);
    }
    for _ in 0..2 {
        let mut last = hue[n - 1];
        for i in 0..n {
            let v = hue[i] * 0.30 + last * 0.70;
            hue[i] = v;
            last = v;
        }
    }
    hue
}

// ─── Perimeter luminance: weighted random distribution ───────────────────────
//
// Per-cell weighted random sampling with slow temporal drift. The ENTIRE
// luminance variation of the border:
//
//   - BURST_RATE (~4%):     luminance ∈ [0.85, 1.0]   ← visibly overdriven
//   - rest      (~96%):    luminance ∈ [0.32, 0.55]   ← perceptible mid
//   - CORNERS:              always BURST              ← corners always pop
//
// Then for each BURST cell, a second roll: NOISE_AT_BURST (~30%) of bursts
// noise-corrupt one adjacent non-burst, non-corner cell to NOISE_LUM (~0.04
// opacity). That's the "imperfect display" texture — a low-opacity dropout
// near a bright moment.
//
// Time drift modulates each cell's burst roll by ±0.04 with a per-cell
// phase, so individual cells slowly drift in and out of overload over
// ~16-second cycles. Geometry is never animated; bright CELLS breathe.

fn compute_perimeter_luminance(
    cells: &[(u16, u16, char)],
    seed: &str,
    time_offset: f32,
) -> (Vec<f32>, Vec<bool>) {
    let n = cells.len();
    let mut lum = vec![0.0_f32; n];
    let mut is_burst = vec![false; n];

    // Pass 1 — per-cell weighted random.
    for i in 0..n {
        let (x, y, gi) = cells[i];
        let h = cell_hash(seed, x, y);
        let static_roll = (h & 0xFFFF) as f32 / 0xFFFF as f32;
        let phase = ((h >> 16) & 0xFFFF) as f32 / 0xFFFF as f32 * std::f32::consts::TAU;
        let drift = (time_offset * 0.4 + phase).sin() * 0.04;
        let roll = (static_roll + drift).clamp(0.0, 1.0);

        let is_corner = matches!(gi, '╭' | '╮' | '╰' | '╯');
        if is_corner || roll < BURST_RATE {
            let detail = ((h >> 32) & 0xFFFF) as f32 / 0xFFFF as f32;
            lum[i] = BURST_LUM_MIN + detail * (BURST_LUM_MAX - BURST_LUM_MIN);
            is_burst[i] = true;
        } else {
            let detail = ((h >> 32) & 0xFFFF) as f32 / 0xFFFF as f32;
            lum[i] = MID_LUM_MIN + detail * (MID_LUM_MAX - MID_LUM_MIN);
        }
    }

    // Pass 2 — burst-triggered noise corruption.
    for i in 0..n {
        if !is_burst[i] {
            continue;
        }
        let (x, y, _) = cells[i];
        let h = cell_hash(seed, x.wrapping_add(7919), y.wrapping_add(7907));
        let noise_roll = (h & 0xFFFF) as f32 / 0xFFFF as f32;
        if noise_roll < NOISE_AT_BURST {
            let direction_left = ((h >> 16) & 1) == 0;
            let neighbor = if direction_left {
                (i + n - 1) % n
            } else {
                (i + 1) % n
            };
            let (_, _, gj) = cells[neighbor];
            let neighbor_corner = matches!(gj, '╭' | '╮' | '╰' | '╯');
            if !is_burst[neighbor] && !neighbor_corner {
                lum[neighbor] = NOISE_LUM;
            }
        }
    }

    (lum, is_burst)
}


// ─── Paint perimeter cells ───────────────────────────────────────────────────
//
// Each cell renders its canonical glyph (`─ │ ╭ ╮ ╰ ╯`) at the luminance
// produced by `compute_perimeter_luminance`. BURST cells get a hue-preserved
// RGB brightening (channels scale up multiplicatively, clip at 255 — so
// magenta becomes brighter magenta, cyan becomes brighter cyan, never
// approaching white). MID and NOISE cells render the cell's natural hue
// at their assigned opacity, no brightening.

fn paint_cells_with_signal(
    buf: &mut Buffer,
    cells: &[(u16, u16, char)],
    hue: &[f32],
    luminance: &[f32],
    seed: &str,
) {
    let p_temp = phase_from(seed, "temp");

    for (i, &(x, y, default_glyph)) in cells.iter().enumerate() {
        let lum = luminance[i];

        // Slight per-cell hue jitter from temp_shift — sub-perceptual color
        // texture so the line doesn't read as flat-color zones.
        let p = i as f32;
        let temp_shift = (p * std::f32::consts::TAU / 80.0 + p_temp).sin() * 0.06;
        let h = (hue[i] + temp_shift).clamp(0.0, 1.0);
        let mut color = lerp_rgb(NEON_MAGENTA, NEON_CYAN, h);

        // Hue-preserved brightening only at BURST tier. Channels clip at
        // 255 individually so saturated magenta/cyan get more saturated;
        // they never wash to white.
        if lum >= BURST_LUM_MIN {
            let factor = 1.0 + (lum - BURST_LUM_MIN) / (BURST_LUM_MAX - BURST_LUM_MIN) * 0.55;
            color = brighten(color, factor);
        }

        let final_color = scale_color(color, lum);
        set_char(buf, x, y, default_glyph, fg(final_color));
    }
}

// ─── Substrate halo at bursts ────────────────────────────────────────────────
//
// One-eighth blocks (`▔ ▁ ▏ ▕`) painted in the substrate cell adjacent to
// each BURST cell. Per-burst hash decides which substrate-side direction(s)
// receive the halo (asymmetric — not every burst halos every direction).
// Uses the burst cell's hue, brightened in-family. Corners halo on both
// perpendicular axes.

fn paint_outward_glow(
    buf: &mut Buffer,
    area: Rect,
    cells: &[(u16, u16, char)],
    hue: &[f32],
    is_burst: &[bool],
    seed: &str,
) {
    let right = area.x + area.width.saturating_sub(1);
    let bottom = area.y + area.height.saturating_sub(1);
    let p_temp = phase_from(seed, "temp");

    for (i, &(x, y, glyph)) in cells.iter().enumerate() {
        if !is_burst[i] {
            continue;
        }

        let on_top = y == area.y;
        let on_bot = y == bottom;
        let on_left = x == area.x;
        let on_right = x == right;
        let is_corner = matches!(glyph, '╭' | '╮' | '╰' | '╯');

        let mut targets: Vec<(i32, i32)> = Vec::with_capacity(2);
        if on_top {
            targets.push((0, -1));
        }
        if on_bot {
            targets.push((0, 1));
        }
        if on_left {
            targets.push((-1, 0));
        }
        if on_right {
            targets.push((1, 0));
        }
        if targets.is_empty() {
            continue;
        }

        let p = i as f32;
        let temp_shift = (p * std::f32::consts::TAU / 80.0 + p_temp).sin() * 0.06;
        let h = (hue[i] + temp_shift).clamp(0.0, 1.0);
        let base_color = lerp_rgb(NEON_MAGENTA, NEON_CYAN, h);

        let cell_h = cell_hash(seed, x, y);
        for (k, &(dx, dy)) in targets.iter().enumerate() {
            let roll = ((cell_h.wrapping_shr(8 * k as u32)) & 0xFF) as f32 / 0xFF as f32;
            // Corners halo on both axes; mid-edge bursts skip ~30% of directions
            // to keep the halo asymmetric.
            if !is_corner && roll < 0.30 {
                continue;
            }

            let nx = x as i32 + dx;
            let ny = y as i32 + dy;
            if nx < 0 || ny < 0 {
                continue;
            }
            let nx = nx as u16;
            let ny = ny as u16;
            let ba = buf.area;
            if nx < ba.x || ny < ba.y || nx >= ba.x + ba.width || ny >= ba.y + ba.height {
                continue;
            }
            if let Some(c) = buf.cell((nx, ny)) {
                let sym = c.symbol();
                let first = sym.chars().next().unwrap_or(' ');
                if first != ' ' && first != '\0' {
                    continue;
                }
            }

            // Halo in the burst's own hue, faintly. ~12% intensity.
            let glow_color = scale_color(base_color, 0.12);
            let glyph = match (dx, dy) {
                (0, -1) => '▔',
                (0, 1) => '▁',
                (-1, 0) => '▕',
                (1, 0) => '▏',
                _ => continue,
            };
            set_char(buf, nx, ny, glyph, fg(glow_color));
        }
    }
}


// ─── Phase 7: temporal phase offset ──────────────────────────────────────────

use std::cell::Cell as StdCell;
thread_local! {
    static TIME_OFFSET: StdCell<f32> = const { StdCell::new(0.0) };
}

/// Set the global per-frame time offset. Called from `tui::run` once per
/// frame with `app.started.elapsed().as_secs_f32()`. Read by the signal
/// computation to drift the shimmer phase over time.
pub fn set_time_offset(t: f32) {
    TIME_OFFSET.with(|c| c.set(t));
}

fn time_offset() -> f32 {
    TIME_OFFSET.with(|c| c.get())
}

// ─── Phase 6: substrate phosphor noise ───────────────────────────────────────
//
// Painted at frame level after the BG block fill, before any panel chrome.
// Each cell at (x, y) gets a deterministic-hash-derived RGB perturbation of
// ±1 unit. Below the conscious threshold — the dark areas should feel deep
// and empty, NOT textured. Higher amplitudes start reading as surface grain
// which kills the "optically thick darkness" feel.

pub fn paint_substrate_noise(buf: &mut Buffer) {
    let ba = buf.area;
    let bg_r = 0x02_i32;
    let bg_g = 0x05_i32;
    let bg_b = 0x0e_i32;
    for y in ba.y..(ba.y + ba.height) {
        for x in ba.x..(ba.x + ba.width) {
            let h = cell_hash("substrate", x, y);
            let rr = ((h & 0xFF) as i32) - 128; // -128..127
            // ±1 unit max in any channel — subliminal, not visible as texture.
            let dr = rr / 128;
            let dg = (((h >> 8) & 0xFF) as i32 - 128) / 128;
            let db = (((h >> 16) & 0xFF) as i32 - 128) / 128;
            let r = (bg_r + dr).clamp(0, 255) as u8;
            let g = (bg_g + dg).clamp(0, 255) as u8;
            let b = (bg_b + db).clamp(0, 255) as u8;
            if let Some(cell) = buf.cell_mut((x, y)) {
                cell.set_bg(Color::Rgb(r, g, b));
            }
        }
    }
}

/// Paint a rectangular signal-border around `area` — same thematics as
/// the panel chrome. Used for the active range chip's outline and any
/// other small bounded element that wants the full energized border.
pub fn signal_rect(buf: &mut Buffer, area: Rect, seed: &str) {
    if area.width < 2 || area.height < 2 {
        return;
    }
    let cells = collect_perimeter(area);
    paint_signal(buf, &cells, seed);
}

/// Solid-color rounded-corner rectangle. Same geometry as `signal_rect`
/// (rounded `╭ ╮ ╰ ╯` corners + `─ │` edges) but painted in one uniform
/// color. Used for elements that want crisp button-style edges instead
/// of the energized streaky chrome — e.g. the active range chip.
pub fn solid_rect(buf: &mut Buffer, area: Rect, color: Color) {
    if area.width < 2 || area.height < 2 {
        return;
    }
    let cells = collect_perimeter(area);
    let style = Style::default().fg(color);
    for &(x, y, glyph) in &cells {
        set_char(buf, x, y, glyph, style);
    }
}

/// Paint a vertical divider strip at column `x`, from `y` for `height`
/// cells. Same signal thematics as panel borders so dividers feel like
/// they belong to the same chrome family.
pub fn signal_divider_v(buf: &mut Buffer, x: u16, y: u16, height: u16, seed: &str) {
    if height < 2 {
        return;
    }
    let cells: Vec<(u16, u16, char)> = (0..height).map(|i| (x, y + i, '│')).collect();
    paint_signal(buf, &cells, seed);
}

/// Walk the panel perimeter clockwise starting at top-left, returning the
/// cell coordinates and default glyph for each step. The returned list is
/// what the energy/hue signals are sampled against; treating it as a single
/// sequence (not four edges) is what lets streaks bleed across corners.
fn collect_perimeter(area: Rect) -> Vec<(u16, u16, char)> {
    let right = area.x + area.width - 1;
    let bottom = area.y + area.height - 1;
    let mut cells = Vec::new();

    cells.push((area.x, area.y, '╭'));
    for x in (area.x + 1)..right {
        cells.push((x, area.y, '─'));
    }
    cells.push((right, area.y, '╮'));
    for y in (area.y + 1)..bottom {
        cells.push((right, y, '│'));
    }
    cells.push((right, bottom, '╯'));
    for x in ((area.x + 1)..right).rev() {
        cells.push((x, bottom, '─'));
    }
    cells.push((area.x, bottom, '╰'));
    for y in ((area.y + 1)..bottom).rev() {
        cells.push((area.x, y, '│'));
    }
    cells
}

fn phase_from(seed: &str, tag: &str) -> f32 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    seed.hash(&mut h);
    tag.hash(&mut h);
    let v = h.finish();
    (v as f32 / u64::MAX as f32) * std::f32::consts::TAU
}

fn brighten(c: Color, factor: f32) -> Color {
    let (r, g, b) = rgb(c);
    Color::Rgb(
        ((r as f32 * factor) as u32).min(255) as u8,
        ((g as f32 * factor) as u32).min(255) as u8,
        ((b as f32 * factor) as u32).min(255) as u8,
    )
}

fn lerp_rgb(from: Color, to: Color, t: f32) -> Color {
    let (fr, fg, fb) = rgb(from);
    let (tr, tg, tb) = rgb(to);
    Color::Rgb(
        (fr as f32 + (tr as f32 - fr as f32) * t) as u8,
        (fg as f32 + (tg as f32 - fg as f32) * t) as u8,
        (fb as f32 + (tb as f32 - fb as f32) * t) as u8,
    )
}

/// Hash (label || x || y) → u64 using std SipHash. Stable within a process
/// run, randomized across runs — exactly what we want for "per-session
/// noise pattern that doesn't change between redraws".
fn cell_hash(label: &str, x: u16, y: u16) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    label.hash(&mut h);
    x.hash(&mut h);
    y.hash(&mut h);
    h.finish()
}

/// Take 16 bits of the hash starting at `shift`, scale to [0, 1).
fn roll(h: u64, shift: u32) -> f32 {
    ((h.wrapping_shr(shift)) & 0xFFFF) as f32 / 0x1_0000_u32 as f32
}

/// Multiply an RGB color by a brightness factor. Channels clamp on the way
/// down (since we only scale by ≤ 1.0). Hue is preserved.
fn scale_color(c: Color, factor: f32) -> Color {
    let (r, g, b) = rgb(c);
    Color::Rgb(
        (r as f32 * factor) as u8,
        (g as f32 * factor) as u8,
        (b as f32 * factor) as u8,
    )
}

/// Build a per-panel seed string from the panel's position. Two panels at
/// different positions → different noise patterns; the same panel at the
/// same position → same noise across redraws.
fn panel_label_seed(_buf_area: Rect, panel_area: Rect) -> String {
    format!("p{}-{}-{}-{}", panel_area.x, panel_area.y, panel_area.width, panel_area.height)
}

fn fg(color: Color) -> Style {
    Style::default().fg(color)
}

fn rgb(c: Color) -> (u8, u8, u8) {
    match c {
        Color::Rgb(r, g, b) => (r, g, b),
        _ => (0xff, 0xff, 0xff),
    }
}

fn set_char(buf: &mut Buffer, x: u16, y: u16, ch: char, style: Style) {
    let a = buf.area;
    if x < a.x || y < a.y || x >= a.x + a.width || y >= a.y + a.height {
        return; // out-of-bounds — silent no-op (don't panic)
    }
    let cell = &mut buf[(x, y)];
    cell.set_char(ch);
    cell.set_style(style);
}

/// Outlined pink chip — used for the active range preset. Single-line: just
/// brackets left/right; multi-line: thin pink rectangle.
pub fn outline_rect(f: &mut Frame, area: Rect) {
    if area.width < 2 || area.height < 1 {
        return;
    }
    let buf = f.buffer_mut();
    let pink = Style::default().fg(PINK);
    let right = area.x + area.width - 1;
    let bottom = area.y + area.height - 1;
    if area.height == 1 {
        set_char(buf, area.x, area.y, '│', pink);
        set_char(buf, right, area.y, '│', pink);
    } else {
        set_char(buf, area.x, area.y, '┌', pink);
        set_char(buf, right, area.y, '┐', pink);
        set_char(buf, area.x, bottom, '└', pink);
        set_char(buf, right, bottom, '┘', pink);
        for x in (area.x + 1)..right {
            set_char(buf, x, area.y, '─', pink);
            set_char(buf, x, bottom, '─', pink);
        }
        for y in (area.y + 1)..bottom {
            set_char(buf, area.x, y, '│', pink);
            set_char(buf, right, y, '│', pink);
        }
    }
}
