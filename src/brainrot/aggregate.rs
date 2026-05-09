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

/// Ordinary-least-squares slope of y on x. 0 when degenerate. Kept for
/// future regime-change detection on inter-arrival gaps; not currently
/// wired into any score, but cheap to retain.
#[allow(dead_code)]
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

    // Driver kinetics: user-typed chars per minute, computed as a robust
    // distribution over per-record chars/min slots. Only populated from
    // records that have the new u_ch field (post-schema bump); older
    // records contribute u_ch=0, which we filter out to avoid biasing
    // the baseline downward.
    pub user_chars_per_min_med: f64,
    pub user_chars_per_min_mad: f64,
    pub n_records_with_u_ch: u64,

    // Latency-tier percentiles (for dynamic word labels). Computed from
    // baseline ms_per_token distribution so each user gets thresholds
    // calibrated to their own normal.
    pub lat_p20: f64,
    pub lat_p40: f64,
    pub lat_p60: f64,
    pub lat_p80: f64,
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

        // Driver kinetics: per-record chars/min slot. Each record represents
        // one API request, which packages a single human turn (or zero
        // chars when it's a tool-loop continuation). The "minute" denominator
        // is the time gap from the previous record in the same session,
        // floored at 1s so a burst doesn't divide by zero. We keep only
        // records where u_ch > 0 — that filters out historical records
        // (pre-schema-bump) AND tool-loop continuations.
        let mut u_chars_per_min: Vec<f64> = Vec::new();
        let mut n_records_with_u_ch = 0u64;
        let by_sid_for_uch = group_records_by_sid_basic(records);
        for (_sid, mut idxs) in by_sid_for_uch {
            idxs.sort_by(|a, b| {
                records[*a]
                    .ts
                    .partial_cmp(&records[*b].ts)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            let mut prev_ts: Option<f64> = None;
            for i in idxs {
                let r = &records[i];
                if r.u_ch > 0 {
                    n_records_with_u_ch += 1;
                    let gap_s = match prev_ts {
                        Some(p) => (r.ts - p).max(1.0),
                        None => 60.0, // first turn of a session: assume ~1min
                    };
                    let chars_per_min = r.u_ch as f64 / (gap_s / 60.0);
                    u_chars_per_min.push(chars_per_min);
                }
                prev_ts = Some(r.ts);
            }
        }
        let user_chars_per_min_med = median(&u_chars_per_min);
        let user_chars_per_min_mad = mad(&u_chars_per_min, user_chars_per_min_med);

        // Latency-tier percentiles (lat in ms across all records)
        let mut lats: Vec<f64> = records.iter().map(|r| r.lat as f64).collect();
        lats.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let lat_p20 = quantile(&mut lats.clone(), 0.20);
        let lat_p40 = quantile(&mut lats.clone(), 0.40);
        let lat_p60 = quantile(&mut lats.clone(), 0.60);
        let lat_p80 = quantile(&mut lats.clone(), 0.80);

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
            user_chars_per_min_med,
            user_chars_per_min_mad,
            n_records_with_u_ch,
            lat_p20,
            lat_p40,
            lat_p60,
            lat_p80,
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

/// Index-only group-by-sid (avoids cloning Records when the caller only
/// needs to walk indices into the original slice).
fn group_records_by_sid_basic(records: &[Record]) -> HashMap<String, Vec<usize>> {
    let mut m: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, r) in records.iter().enumerate() {
        let sid = r.sid.clone().unwrap_or_else(|| "_orphan".into());
        m.entry(sid).or_default().push(i);
    }
    m
}

// ─── Driver-side (human-input kinetics) ──────────────────────────────────────
//
// Driver score measures the kinetic load the human is putting on the system:
// how many user-typed characters per minute are being produced, summed
// cumulatively across all active sessions in the window. Parallel sessions
// stack additively — driving 4 simultaneously is 4× the brain-burn, not
// the average per session.
//
// The metric ignores tool-loop continuations (which are bot-driven, not
// human-driven). It distinguishes them via the per-record u_ch field
// captured at request time: u_ch > 0 means the last user message of that
// request was plain text (fresh human input); u_ch == 0 means it was a
// tool_result (bot continuation) or the record predates the schema bump.
//
// When a window has too few new-schema records (< MIN_UCH_RECORDS), the
// driver score returns a neutral 50 with an "insufficient data" signal —
// it doesn't pretend to know.

const MIN_UCH_RECORDS_WINDOW: u64 = 3;
const MIN_UCH_RECORDS_BASELINE: u64 = 10;

/// Whether the driver-score baseline has accumulated enough new-schema
/// records to score against. Callers use this to render the driver tile
/// as "—" rather than a misleading neutral 50, and to omit the driver
/// line from charts entirely when bootstrapping.
pub fn driver_is_bootstrapping(baseline: &Baseline) -> bool {
    baseline.n_records_with_u_ch < MIN_UCH_RECORDS_BASELINE
}

fn driver_chars_per_min(a: &Aggregate) -> Option<f64> {
    // Sum u_ch within the window, divide by active span. This is the
    // cumulative kinetic — parallel sessions naturally add up because we
    // sum the chars regardless of which session contributed them.
    let total_u_ch: u64 = a.records.iter().map(|r| r.u_ch).sum();
    let with_u_ch: u64 = a.records.iter().filter(|r| r.u_ch > 0).count() as u64;
    if with_u_ch < MIN_UCH_RECORDS_WINDOW {
        return None;
    }
    let first = a.first_ts?;
    let last = a.last_ts.unwrap_or(first);
    let span_min = ((last - first) / 60.0).max(1.0);
    Some(total_u_ch as f64 / span_min)
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
    // Insufficient new-schema baseline → can't z-score against history.
    // Insufficient new-schema window → can't compute current rate.
    // Both cases return neutral 50.
    if baseline.n_records_with_u_ch < MIN_UCH_RECORDS_BASELINE {
        return 50;
    }
    let Some(cur_cpm) = driver_chars_per_min(a) else {
        return 50;
    };
    // Floor MAD so a too-tight baseline can't make z explode.
    let mad_floor = (baseline.user_chars_per_min_med * 0.20).max(5.0);
    let mad = baseline.user_chars_per_min_mad.max(mad_floor);
    let z = robust_z(cur_cpm, baseline.user_chars_per_min_med, mad);
    let raw = logistic_score(z, 1.5);
    let shrunk = shrink(raw, confidence(a.n));
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

/// Diagnostic dump of every score component for one window. Use it to
/// validate that the headline numbers come from the components you expect.
pub fn score_breakdown(
    a: &Aggregate,
    baseline: &Baseline,
) -> ScoreBreakdown {
    let conf = confidence(a.n);

    // Driver: kinetic chars/min vs baseline median.
    let total_u_ch: u64 = a.records.iter().map(|r| r.u_ch).sum();
    let with_u_ch: u64 = a.records.iter().filter(|r| r.u_ch > 0).count() as u64;
    let cur_cpm = driver_chars_per_min(a).unwrap_or(0.0);
    let mad_floor = (baseline.user_chars_per_min_med * 0.20).max(5.0);
    let used_mad = baseline.user_chars_per_min_mad.max(mad_floor);
    let driver_z = if used_mad > 1e-9 {
        (cur_cpm - baseline.user_chars_per_min_med) / used_mad
    } else {
        0.0
    };
    let d_raw = logistic_score(driver_z, 1.5);
    let d_shrunk = shrink(d_raw, conf);

    let b_brevity = bot_brevity(a, baseline);
    let b_stalling = bot_stalling(a, baseline);
    let b_wandering = bot_wandering(a, baseline);
    let b_cache_drag = bot_cache_drag(a, baseline);
    let b_raw =
        b_brevity * 0.35 + b_stalling * 0.25 + b_wandering * 0.25 + b_cache_drag * 0.15;

    ScoreBreakdown {
        n: a.n,
        confidence: conf,
        d_total_u_ch: total_u_ch,
        d_with_u_ch: with_u_ch,
        d_chars_per_min: cur_cpm,
        d_baseline_cpm: baseline.user_chars_per_min_med,
        d_baseline_mad: used_mad,
        d_z: driver_z,
        d_raw, d_shrunk,
        b_brevity, b_stalling, b_wandering, b_cache_drag,
        b_raw, b_shrunk: shrink(b_raw, conf),
    }
}

#[derive(Debug)]
pub struct ScoreBreakdown {
    pub n: u64,
    pub confidence: f64,
    pub d_total_u_ch: u64,
    pub d_with_u_ch: u64,
    pub d_chars_per_min: f64,
    pub d_baseline_cpm: f64,
    pub d_baseline_mad: f64,
    pub d_z: f64,
    pub d_raw: f64,
    pub d_shrunk: f64,
    pub b_brevity: f64,
    pub b_stalling: f64,
    pub b_wandering: f64,
    pub b_cache_drag: f64,
    pub b_raw: f64,
    pub b_shrunk: f64,
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
