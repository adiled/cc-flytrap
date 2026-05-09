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

// ─── Driver-side (V·V·P) ─────────────────────────────────────────────────────

fn driver_bloat(a: &Aggregate) -> f64 {
    let avg_in = if a.n > 0 { a.r#in as f64 / a.n as f64 } else { 0.0 };
    ((avg_in - 1000.0) / 200.0).clamp(0.0, 30.0)
}

fn driver_rapidfire(a: &Aggregate) -> f64 {
    let mut g = a.gaps();
    if g.is_empty() {
        return 0.0;
    }
    let med = quantile(&mut g, 0.5);
    if med >= 90.0 {
        0.0
    } else {
        ((90.0 - med) / 3.6).min(25.0)
    }
}

fn driver_burst(a: &Aggregate) -> f64 {
    let mut g = a.gaps();
    if g.len() < 5 {
        return 0.0;
    }
    let p10 = quantile(&mut g, 0.1);
    let med = quantile(&mut g, 0.5);
    if med <= 0.0 {
        return 0.0;
    }
    let ratio = p10 / med;
    ((0.3 - ratio) / 0.3 * 20.0).clamp(0.0, 20.0)
}

fn driver_sprawl(a: &Aggregate) -> f64 {
    let Some(first) = a.first_ts else { return 0.0 };
    let last = a.last_ts.unwrap_or(first);
    let span_h = ((last - first) / 3600.0).max(1.0 / 60.0);
    let sess_count = a.sessions.len().max(1) as f64;
    let sess_per_hour = sess_count / span_h;
    let sprawl = ((sess_per_hour - 2.0) * 5.0).clamp(0.0, 15.0);
    let n_models = a.models.len() as f64;
    let thrash = if a.n > 5 {
        ((n_models - 2.0) * 5.0).clamp(0.0, 10.0)
    } else {
        0.0
    };
    sprawl + thrash
}

pub fn driver_score(a: &Aggregate) -> u32 {
    if a.n == 0 {
        return 0;
    }
    let s = driver_bloat(a) + driver_rapidfire(a) + driver_burst(a) + driver_sprawl(a);
    s.min(100.0) as u32
}

// ─── Bot-side (L·L·V·V) ──────────────────────────────────────────────────────

fn bot_choke(a: &Aggregate) -> f64 {
    let pairs: Vec<(f64, f64)> = a
        .records
        .iter()
        .map(|r| (r.r#in as f64, r.lat as f64))
        .collect();
    let s = slope(&pairs);
    ((s - 1.0) * 7.5).clamp(0.0, 30.0)
}

fn bot_spike(a: &Aggregate) -> f64 {
    if a.lats.is_empty() {
        return 0.0;
    }
    let mut lats: Vec<f64> = a.lats.iter().map(|x| *x as f64).collect();
    let p50 = quantile(&mut lats.clone(), 0.5);
    let p99 = quantile(&mut lats, 0.99);
    if p50 <= 0.0 {
        return 0.0;
    }
    let ratio = p99 / p50;
    ((ratio - 5.0) * 1.7).clamp(0.0, 25.0)
}

fn bot_collapse(a: &Aggregate) -> f64 {
    let avg_out = if a.n > 0 { a.out as f64 / a.n as f64 } else { 0.0 };
    if avg_out >= 200.0 {
        0.0
    } else {
        ((200.0 - avg_out) / 8.0).min(25.0)
    }
}

fn bot_hedge(a: &Aggregate) -> f64 {
    let ratio = if a.r#in > 0 {
        a.out as f64 / a.r#in as f64
    } else {
        0.0
    };
    if ratio >= 0.15 {
        0.0
    } else {
        ((0.15 - ratio) / 0.0075).min(20.0)
    }
}

pub fn bot_score(a: &Aggregate) -> u32 {
    if a.n == 0 {
        return 0;
    }
    let s = bot_choke(a) + bot_spike(a) + bot_collapse(a) + bot_hedge(a);
    s.min(100.0) as u32
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
