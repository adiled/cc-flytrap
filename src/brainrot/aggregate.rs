//! Record aggregation + bot/driver scoring (V·L·P·V signal model).
//! Ports `aggregate()`, `bot_score`, `driver_score`, and the `_*` helpers
//! from cc-flytrap/brainrot.py. Math is identical (no content inspection,
//! only behaviour from the ledger telemetry).

use crate::ledger_read::Record;
use std::collections::{HashMap, HashSet};

#[derive(Default, Debug)]
pub struct Aggregate {
    pub n: u64,
    pub r#in: u64,
    pub out: u64,
    pub tot: u64,
    pub lat_sum: u64,
    pub lat_max: u64,
    pub lats: Vec<u64>,
    pub first_ts: Option<f64>,
    pub last_ts: Option<f64>,
    pub models: HashMap<String, u64>,
    pub sessions: HashSet<String>,
    pub by_hour: HashMap<u8, HourBucket>,
    pub by_minute: HashMap<i64, MinuteBucket>,
    pub records: Vec<Record>,
}

#[derive(Default, Debug)]
pub struct HourBucket {
    pub n: u64,
    pub tot: u64,
    pub lat_sum: u64,
}

#[derive(Default, Debug)]
pub struct MinuteBucket {
    pub n: u64,
    pub tot: u64,
}

impl Aggregate {
    pub fn ingest<I: IntoIterator<Item = Record>>(records: I) -> Self {
        let mut a = Aggregate::default();
        for r in records {
            a.n += 1;
            a.r#in += r.r#in;
            a.out += r.out;
            a.tot += r.tot;
            a.lat_sum += r.lat;
            if r.lat > a.lat_max { a.lat_max = r.lat; }
            a.lats.push(r.lat);

            let ts = r.ts;
            a.first_ts = Some(a.first_ts.map_or(ts, |x| x.min(ts)));
            a.last_ts = Some(a.last_ts.map_or(ts, |x| x.max(ts)));

            let model = r.model.clone().unwrap_or_else(|| "unknown".into());
            *a.models.entry(model).or_insert(0) += 1;
            if let Some(s) = &r.sid {
                a.sessions.insert(s.clone());
            }

            let hour = ((ts as i64).rem_euclid(86400) / 3600) as u8;
            let hb = a.by_hour.entry(hour).or_default();
            hb.n += 1;
            hb.tot += r.tot;
            hb.lat_sum += r.lat;

            let minute = (ts as i64) / 60;
            let mb = a.by_minute.entry(minute).or_default();
            mb.n += 1;
            mb.tot += r.tot;

            a.records.push(r);
        }
        a
    }

    fn gaps(&self) -> Vec<f64> {
        let mut ts: Vec<f64> = self.records.iter().map(|r| r.ts).collect();
        ts.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        ts.windows(2).map(|w| w[1] - w[0]).collect()
    }
}

/// Driver-vs-bot turn classification.
///
/// `Driver` = first request of a session OR any request following a gap >
/// `BOT_LOOP_THRESHOLD` seconds (5s by default). `Bot` = anything else
/// (continuation of a tool-loop).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TurnKind {
    Driver,
    Bot,
}

pub const BOT_LOOP_THRESHOLD: f64 = 5.0;

/// Classify each record as Driver or Bot. Returns a vec aligned 1:1 with
/// the input slice. Stable: walks each session in chronological order and
/// inspects the inter-arrival gap from the previous response end.
pub fn classify_turns(records: &[Record]) -> Vec<TurnKind> {
    let mut kinds = vec![TurnKind::Driver; records.len()];
    let mut by_sid: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, r) in records.iter().enumerate() {
        let sid = r.sid.clone().unwrap_or_else(|| "_orphan".into());
        by_sid.entry(sid).or_default().push(i);
    }
    for (_sid, mut idxs) in by_sid {
        idxs.sort_by(|a, b| {
            records[*a]
                .ts
                .partial_cmp(&records[*b].ts)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let mut prev_te: Option<f64> = None;
        for i in &idxs {
            let r = &records[*i];
            let te = if r.te > 0.0 { r.te } else { r.ts };
            kinds[*i] = match prev_te {
                None => TurnKind::Driver,
                Some(prev) if (r.ts - prev) > BOT_LOOP_THRESHOLD => TurnKind::Driver,
                Some(_) => TurnKind::Bot,
            };
            prev_te = Some(te);
        }
    }
    kinds
}

fn quantile(xs: &mut [f64], q: f64) -> f64 {
    if xs.is_empty() {
        return 0.0;
    }
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let k = (xs.len() - 1) as f64 * q;
    let f = k.floor() as usize;
    let c = (f + 1).min(xs.len() - 1);
    xs[f] + (xs[c] - xs[f]) * (k - f as f64)
}

/// Ordinary-least-squares slope of y on x. 0 when degenerate.
fn slope(pairs: &[(f64, f64)]) -> f64 {
    let n = pairs.len();
    if n < 3 {
        return 0.0;
    }
    let nf = n as f64;
    let sx: f64 = pairs.iter().map(|(x, _)| x).sum();
    let sy: f64 = pairs.iter().map(|(_, y)| y).sum();
    let sxx: f64 = pairs.iter().map(|(x, _)| x * x).sum();
    let sxy: f64 = pairs.iter().map(|(x, y)| x * y).sum();
    let denom = nf * sxx - sx * sx;
    if denom == 0.0 {
        0.0
    } else {
        (nf * sxy - sx * sy) / denom
    }
}

// ─── Robust statistics ───────────────────────────────────────────────────────

fn median(xs: &[f64]) -> f64 {
    if xs.is_empty() {
        return 0.0;
    }
    let mut s = xs.to_vec();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = s.len();
    if n.is_multiple_of(2) {
        (s[n / 2 - 1] + s[n / 2]) / 2.0
    } else {
        s[n / 2]
    }
}

/// Median Absolute Deviation, scaled to be comparable to stdev for a normal
/// distribution (×1.4826). Robust to outliers — a single 100-second tool loop
/// doesn't distort the dispersion estimate the way stdev would.
fn mad(xs: &[f64], med: f64) -> f64 {
    if xs.is_empty() {
        return 0.0;
    }
    let dev: Vec<f64> = xs.iter().map(|x| (x - med).abs()).collect();
    median(&dev) * 1.4826
}

/// Coefficient of variation = stdev / |mean|. Returns 0 for degenerate input.
fn cv(xs: &[f64]) -> f64 {
    if xs.len() < 2 {
        return 0.0;
    }
    let n = xs.len() as f64;
    let mean = xs.iter().sum::<f64>() / n;
    let var = xs.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n;
    let stdev = var.sqrt();
    if mean.abs() < 1e-9 {
        0.0
    } else {
        stdev / mean
    }
}

/// Robust z-score: `(x - median) / MAD`. Returns 0 when MAD is degenerate
/// (no dispersion in the baseline) so we don't over-claim signal.
fn robust_z(x: f64, med: f64, mad: f64) -> f64 {
    if mad <= 1e-9 {
        return 0.0;
    }
    (x - med) / mad
}

/// Map a robust z-score to a 0..100 distress score:
///   z = 0          →  50  (at baseline / typical)
///   z = +scale     →  ~88 (notably worse than usual)
///   z = -scale     →  ~12 (notably better than usual)
///   z → ±∞         →  100 / 0 (saturates gracefully)
///
/// Convention: positive z means "more concerning" — each component flips its
/// sign to ensure that holds (e.g., bot brevity uses `baseline - current` so
/// shorter-than-usual output produces positive z).
fn logistic_score(z: f64, scale: f64) -> f64 {
    50.0 + 50.0 * (z / scale).tanh()
}

// ─── Baseline: the user's historical fingerprint ─────────────────────────────
//
// Computed once from the full ledger (or whatever set of records the caller
// provides). Subsequent score computations on a window are z-scored against
// this fingerprint. So "high score" means "this window is unusual for YOU,"
// not "this window crosses some absolute threshold guessed at design time."

#[derive(Default, Debug, Clone)]
pub struct Baseline {
    pub n_records: u64,
    pub n_sessions: usize,

    // Per-record metric distributions
    pub out_med: f64,
    pub out_mad: f64,
    pub in_med: f64,
    pub in_mad: f64,
    pub ms_per_token_med: f64,
    pub ms_per_token_mad: f64,

    // Cache miss rate (single scalar)
    pub cache_miss_rate: f64,

    // Per-session statistic distributions
    pub session_out_cv_med: f64,
    pub session_out_cv_mad: f64,
    pub session_models_med: f64,
    pub session_models_mad: f64,
    pub gap_cv_med: f64,
    pub gap_cv_mad: f64,

    // Window-rate scalar (sessions/hour over the entire baseline span)
    pub sessions_per_hour: f64,
}

impl Baseline {
    /// Empty baseline — used when no historical data exists yet (brand-new
    /// install). Score functions interpret this as "no signal" and return 0.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Build a baseline fingerprint from an arbitrary record set. Typically
    /// called with the entire ledger so subsequent windowed scores express
    /// "deviation from your typical behavior."
    pub fn from_records(records: &[Record]) -> Self {
        if records.is_empty() {
            return Self::default();
        }

        // Per-record arrays
        let outs: Vec<f64> = records.iter().map(|r| r.out as f64).collect();
        let ins: Vec<f64> = records.iter().map(|r| r.r#in as f64).collect();
        let ms_per_token: Vec<f64> = records
            .iter()
            .filter(|r| r.out > 0)
            .map(|r| r.lat as f64 / r.out as f64)
            .collect();

        let out_med = median(&outs);
        let out_mad = mad(&outs, out_med);
        let in_med = median(&ins);
        let in_mad = mad(&ins, in_med);
        let ms_per_token_med = median(&ms_per_token);
        let ms_per_token_mad = mad(&ms_per_token, ms_per_token_med);

        // Cache miss rate: cc / (cc + cr) globally. Single scalar — score
        // functions use a synthetic MAD around it for z-scoring.
        let total_cr: u64 = records.iter().map(|r| r.cr).sum();
        let total_cc: u64 = records.iter().map(|r| r.cc).sum();
        let cache_miss_rate = if total_cr + total_cc > 0 {
            total_cc as f64 / (total_cr + total_cc) as f64
        } else {
            0.0
        };

        // Per-session metrics
        let by_sid = group_records_by_sid(records);
        let n_sessions = by_sid.len();

        let mut session_out_cvs: Vec<f64> = Vec::new();
        let mut session_models: Vec<f64> = Vec::new();
        let mut session_gap_cvs: Vec<f64> = Vec::new();

        for recs in by_sid.values() {
            // Output-size CV within session (bot wandering)
            let outs: Vec<f64> = recs.iter().map(|r| r.out as f64).collect();
            session_out_cvs.push(cv(&outs));

            // Unique models within session (driver thrash)
            let mut models: HashSet<String> = HashSet::new();
            for r in recs {
                if let Some(m) = &r.model {
                    models.insert(m.clone());
                }
            }
            session_models.push(models.len() as f64);

            // Inter-arrival gap CV within session (driver pace volatility)
            let mut sorted = recs.clone();
            sorted.sort_by(|a, b| {
                a.ts.partial_cmp(&b.ts).unwrap_or(std::cmp::Ordering::Equal)
            });
            if sorted.len() >= 3 {
                let gaps: Vec<f64> =
                    sorted.windows(2).map(|w| w[1].ts - w[0].ts).collect();
                session_gap_cvs.push(cv(&gaps));
            }
        }

        let session_out_cv_med = median(&session_out_cvs);
        let session_out_cv_mad = mad(&session_out_cvs, session_out_cv_med);
        let session_models_med = median(&session_models);
        let session_models_mad = mad(&session_models, session_models_med);
        let gap_cv_med = median(&session_gap_cvs);
        let gap_cv_mad = mad(&session_gap_cvs, gap_cv_med);

        // Sessions per hour over the full baseline span
        let first_ts = records.iter().map(|r| r.ts).fold(f64::INFINITY, f64::min);
        let last_ts = records
            .iter()
            .map(|r| r.ts)
            .fold(f64::NEG_INFINITY, f64::max);
        let span_hours = ((last_ts - first_ts) / 3600.0).max(1.0 / 60.0);
        let sessions_per_hour = n_sessions as f64 / span_hours;

        Self {
            n_records: records.len() as u64,
            n_sessions,
            out_med,
            out_mad,
            in_med,
            in_mad,
            ms_per_token_med,
            ms_per_token_mad,
            cache_miss_rate,
            session_out_cv_med,
            session_out_cv_mad,
            session_models_med,
            session_models_mad,
            gap_cv_med,
            gap_cv_mad,
            sessions_per_hour,
        }
    }
}

fn group_records_by_sid(records: &[Record]) -> HashMap<String, Vec<Record>> {
    let mut m: HashMap<String, Vec<Record>> = HashMap::new();
    for r in records {
        let sid = r.sid.clone().unwrap_or_else(|| "_orphan".into());
        m.entry(sid).or_default().push(r.clone());
    }
    m
}

// ─── Driver-side (kinetics-focused) ──────────────────────────────────────────
//
// Driver score measures how the human is INPUTTING into the system. Five
// components, each self-normalized against the user's baseline:
//
//   sprawl       — sessions per hour vs typical (high = scattered attention)
//   pace         — CV of inter-arrival gaps within sessions (high = bursty)
//   bloat        — median input tokens per request vs typical
//   thrash       — unique models per session vs typical
//   acceleration — slope of inter-arrival gaps over time (panic vs focus)

fn driver_sprawl(a: &Aggregate, baseline: &Baseline) -> f64 {
    let Some(first) = a.first_ts else { return 50.0 };
    let last = a.last_ts.unwrap_or(first);
    let span_h = ((last - first) / 3600.0).max(1.0 / 60.0);
    let cur = a.sessions.len() as f64 / span_h;
    // Synthetic dispersion: ±40% of baseline, floored at 0.2 sess/h to
    // prevent infinitely large z when baseline is near zero.
    let synth_mad = (baseline.sessions_per_hour * 0.4).max(0.2);
    let z = robust_z(cur, baseline.sessions_per_hour, synth_mad);
    logistic_score(z, 1.5)
}

fn driver_pace(a: &Aggregate, baseline: &Baseline) -> f64 {
    let by_sid = group_records_by_sid(&a.records);
    let cvs: Vec<f64> = by_sid
        .values()
        .filter(|recs| recs.len() >= 3)
        .map(|recs| {
            let mut sorted = recs.clone();
            sorted.sort_by(|a, b| {
                a.ts.partial_cmp(&b.ts).unwrap_or(std::cmp::Ordering::Equal)
            });
            let gaps: Vec<f64> =
                sorted.windows(2).map(|w| w[1].ts - w[0].ts).collect();
            cv(&gaps)
        })
        .collect();
    if cvs.is_empty() {
        return 50.0;
    }
    let cur = median(&cvs);
    let z = robust_z(cur, baseline.gap_cv_med, baseline.gap_cv_mad);
    logistic_score(z, 1.5)
}

fn driver_bloat(a: &Aggregate, baseline: &Baseline) -> f64 {
    if a.records.is_empty() {
        return 50.0;
    }
    let ins: Vec<f64> = a.records.iter().map(|r| r.r#in as f64).collect();
    let cur = median(&ins);
    let z = robust_z(cur, baseline.in_med, baseline.in_mad);
    logistic_score(z, 1.5)
}

fn driver_thrash(a: &Aggregate, baseline: &Baseline) -> f64 {
    let by_sid = group_records_by_sid(&a.records);
    if by_sid.is_empty() {
        return 50.0;
    }
    let counts: Vec<f64> = by_sid
        .values()
        .map(|recs| {
            let mut models: HashSet<String> = HashSet::new();
            for r in recs {
                if let Some(m) = &r.model {
                    models.insert(m.clone());
                }
            }
            models.len() as f64
        })
        .collect();
    let cur = median(&counts);
    let z = robust_z(cur, baseline.session_models_med, baseline.session_models_mad);
    logistic_score(z, 1.5)
}

/// Acceleration: do gaps between requests within sessions trend up (slowing
/// down / focus drift) or down (speeding up / panic mode)? Score uses |z|
/// because either direction is a regime change worth flagging.
fn driver_acceleration(a: &Aggregate) -> f64 {
    let by_sid = group_records_by_sid(&a.records);
    let slopes: Vec<f64> = by_sid
        .values()
        .filter(|recs| recs.len() >= 4)
        .map(|recs| {
            let mut sorted = recs.clone();
            sorted.sort_by(|a, b| {
                a.ts.partial_cmp(&b.ts).unwrap_or(std::cmp::Ordering::Equal)
            });
            let gaps: Vec<f64> =
                sorted.windows(2).map(|w| w[1].ts - w[0].ts).collect();
            let pairs: Vec<(f64, f64)> = gaps
                .iter()
                .enumerate()
                .map(|(i, g)| (i as f64, *g))
                .collect();
            slope(&pairs)
        })
        .collect();
    if slopes.is_empty() {
        return 50.0;
    }
    let cur = median(&slopes);
    let dispersion = mad(&slopes, cur).max(0.5);
    let z = robust_z(cur, 0.0, dispersion).abs();
    logistic_score(z, 1.5)
}

/// Sample-size confidence factor in [0, 1]. With few records the per-window
/// statistics (medians, CVs, z-scores) are essentially noise, and small-n
/// scores can land far from baseline by pure chance. Multiplying the
/// (raw_score − 50) excursion by this factor shrinks scores toward the
/// neutral 50 when n is small, and lets the raw score through once n
/// crosses the saturation threshold.
///
/// Linear ramp from 0 at n=0 to 1 at n=`SAMPLE_FULL`. Tuned so a window
/// with ≥ 50 records gets full weight; a window with 5 records gets only
/// 10% of the deviation.
const SAMPLE_FULL: f64 = 50.0;

fn confidence(n: u64) -> f64 {
    (n as f64 / SAMPLE_FULL).clamp(0.0, 1.0)
}

fn shrink(raw_score: f64, confidence: f64) -> f64 {
    50.0 + (raw_score - 50.0) * confidence
}

pub fn driver_score(a: &Aggregate, baseline: &Baseline) -> u32 {
    if a.n == 0 || baseline.n_records == 0 {
        return 0;
    }
    let sprawl = driver_sprawl(a, baseline);
    let pace = driver_pace(a, baseline);
    let bloat = driver_bloat(a, baseline);
    let thrash = driver_thrash(a, baseline);
    let accel = driver_acceleration(a);
    let composite =
        sprawl * 0.25 + pace * 0.20 + bloat * 0.25 + thrash * 0.15 + accel * 0.15;
    let shrunk = shrink(composite, confidence(a.n));
    shrunk.round().clamp(0.0, 100.0) as u32
}

// ─── Bot-side (output-health-focused) ────────────────────────────────────────
//
// Bot score measures the QUALITY/HEALTH of the bot's outputs and its
// streaming behavior, NOT the upstream API's tail latency. Four components:
//
//   brevity    — median output tokens vs typical (low = bot bailing)
//   stalling   — ms per output token vs typical (high = streaming choke)
//   wandering  — within-session output variance vs typical (high = unstable)
//   cache_drag — cache miss rate vs typical (high = no cache benefit)

fn bot_brevity(a: &Aggregate, baseline: &Baseline) -> f64 {
    if a.records.is_empty() {
        return 50.0;
    }
    let outs: Vec<f64> = a.records.iter().map(|r| r.out as f64).collect();
    let cur = median(&outs);
    // Concerning when current is BELOW baseline → swap sign of diff.
    let z = robust_z(baseline.out_med - cur, 0.0, baseline.out_mad);
    logistic_score(z, 1.5)
}

fn bot_stalling(a: &Aggregate, baseline: &Baseline) -> f64 {
    let ms_per_token: Vec<f64> = a
        .records
        .iter()
        .filter(|r| r.out > 0)
        .map(|r| r.lat as f64 / r.out as f64)
        .collect();
    if ms_per_token.is_empty() {
        return 50.0;
    }
    let cur = median(&ms_per_token);
    // Concerning when current is ABOVE baseline.
    let z = robust_z(cur, baseline.ms_per_token_med, baseline.ms_per_token_mad);
    logistic_score(z, 1.5)
}

fn bot_wandering(a: &Aggregate, baseline: &Baseline) -> f64 {
    let by_sid = group_records_by_sid(&a.records);
    let cvs: Vec<f64> = by_sid
        .values()
        .filter(|recs| recs.len() >= 3)
        .map(|recs| {
            let outs: Vec<f64> = recs.iter().map(|r| r.out as f64).collect();
            cv(&outs)
        })
        .collect();
    if cvs.is_empty() {
        return 50.0;
    }
    let cur = median(&cvs);
    let z = robust_z(cur, baseline.session_out_cv_med, baseline.session_out_cv_mad);
    logistic_score(z, 1.5)
}

fn bot_cache_drag(a: &Aggregate, baseline: &Baseline) -> f64 {
    let total_cr: u64 = a.records.iter().map(|r| r.cr).sum();
    let total_cc: u64 = a.records.iter().map(|r| r.cc).sum();
    if total_cr + total_cc == 0 {
        return 50.0;
    }
    let cur = total_cc as f64 / (total_cr + total_cc) as f64;
    let synth_mad = (baseline.cache_miss_rate * 0.3).max(0.05);
    let z = robust_z(cur, baseline.cache_miss_rate, synth_mad);
    logistic_score(z, 1.5)
}

pub fn bot_score(a: &Aggregate, baseline: &Baseline) -> u32 {
    if a.n == 0 || baseline.n_records == 0 {
        return 0;
    }
    let brevity = bot_brevity(a, baseline);
    let stalling = bot_stalling(a, baseline);
    let wandering = bot_wandering(a, baseline);
    let cache_drag = bot_cache_drag(a, baseline);
    let composite =
        brevity * 0.35 + stalling * 0.25 + wandering * 0.25 + cache_drag * 0.15;
    let shrunk = shrink(composite, confidence(a.n));
    shrunk.round().clamp(0.0, 100.0) as u32
}

// ─── Labels ──────────────────────────────────────────────────────────────────

pub fn vibe_label(score: u32) -> &'static str {
    match score {
        s if s < 20 => "crisp 🧊",
        s if s < 40 => "fine",
        s if s < 60 => "mid",
        s if s < 80 => "cooked 🔥",
        _ => "fried 💀",
    }
}

pub fn diagnosis(bot: u32, driver: u32) -> Option<&'static str> {
    if bot < 30 && driver < 30 {
        return None;
    }
    let diff = bot.abs_diff(driver);
    let avg = (bot + driver) / 2;
    if diff < 15 {
        if avg > 60 {
            return Some("co-rotting — driver and bot are in a feedback loop");
        }
        if avg > 40 {
            return Some("drift on both sides; nothing alarming yet");
        }
        return None;
    }
    if driver > bot {
        if driver > 70 {
            return Some("driver is rotting; bot is keeping up. throttle, refocus.");
        }
        if driver > 50 {
            return Some("prompts are bloating or driver is rapid-firing");
        }
        return Some("driver-side drift; bot is fine");
    }
    if bot > 70 {
        return Some("bot is cooked. swap models or clear context.");
    }
    if bot > 50 {
        return Some("bot output is shrinking or latency is climbing");
    }
    Some("bot-side drift; driver is clean")
}

pub fn short_model(m: &str) -> String {
    if m.is_empty() {
        return "unknown".into();
    }
    let stripped = m.strip_prefix("claude-").unwrap_or(m);
    let parts: Vec<&str> = stripped.split('-').collect();
    if parts.is_empty() {
        return m.into();
    }
    let name = parts[0];
    let ver = if parts.len() >= 3 && parts[1].chars().all(|c| c.is_ascii_digit()) {
        if parts[2].chars().all(|c| c.is_ascii_digit()) {
            format!("-{}.{}", parts[1], parts[2])
        } else {
            format!("-{}", parts[1])
        }
    } else if parts.len() >= 2 && parts[1].chars().all(|c| c.is_ascii_digit()) {
        format!("-{}", parts[1])
    } else {
        String::new()
    };
    format!("{}{}", name, ver)
}
