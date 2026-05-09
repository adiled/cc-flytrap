//! `ccft seed` — replace ledger contents for selected sessions using
//! Claude Code's local session JSONLs at `~/.claude/projects/`.
//!
//! Semantics: **session is the unit of replacement.** For each session the
//! user selects (via `--session` or by date range with `--since/--until`):
//!
//!   1. Drop every existing ledger row for that session.
//!   2. Insert one row per user→assistant pair found in the session JSONL,
//!      with all fields the JSONL provides (sid, ts, te, model, in/out/cr/
//!      cc, lat, u_ch, tr_ch). Network-side metadata (cip, pip, sip, c_us)
//!      is left at defaults — that's data only the live proxy can know.
//!
//! All ledger rows for sessions NOT being seeded are preserved untouched.
//! Final ledger is sorted chronologically by ts.
//!
//! Original ledger is always copied to `ledger.jsonl.bak.<unix-ts>` before
//! any write. Honors `--dry-run`.

use crate::ledger::ledger_path;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

pub struct Args {
    pub session: Option<String>,
    pub since: Option<String>,
    pub until: Option<String>,
    pub dry_run: bool,
}

#[derive(Debug, Clone)]
struct Turn {
    sid: String,
    /// User message timestamp (request start).
    ts: f64,
    /// Assistant response timestamp (request end). None if no response in JSONL.
    te: Option<f64>,
    u_ch: u64,
    tr_ch: u64,
    model: Option<String>,
    in_tok: u64,
    out_tok: u64,
    cr: u64,
    cc: u64,
}

pub fn run(args: Args) -> Result<(), Box<dyn std::error::Error>> {
    if args.session.is_some() && (args.since.is_some() || args.until.is_some()) {
        return Err("--session is mutually exclusive with --since/--until".into());
    }
    if args.session.is_none() && args.since.is_none() && args.until.is_none() {
        return Err("provide --session SID or --since/--until DATE".into());
    }

    let projects_dir = std::env::var("HOME")
        .map(PathBuf::from)
        .map_err(|_| -> Box<dyn std::error::Error> { "no HOME env var".into() })?
        .join(".claude/projects");
    if !projects_dir.exists() {
        return Err(format!("no claude projects dir at {}", projects_dir.display()).into());
    }

    let since = args.since.as_deref().map(parse_when).transpose()?;
    let until = args.until.as_deref().map(parse_when).transpose()?;

    println!("scanning session JSONLs at {}", projects_dir.display());
    let all_turns = collect_all_turns(&projects_dir, args.session.as_deref())?;
    println!("scanned {} turns total", all_turns.len());

    // Pick which sessions to operate on:
    //   --session SID:           that one session
    //   --since/--until:         every session that has at least one
    //                            paired turn (te is Some) within the range
    let affected: HashSet<String> = if let Some(sid) = args.session.as_deref() {
        std::iter::once(sid.to_string()).collect()
    } else {
        all_turns
            .iter()
            .filter(|t| t.te.is_some())
            .filter(|t| since.map(|s| t.ts >= s).unwrap_or(true))
            .filter(|t| until.map(|u| t.ts <= u).unwrap_or(true))
            .map(|t| t.sid.clone())
            .collect()
    };
    println!("affected sessions: {}", affected.len());
    if affected.is_empty() {
        println!("nothing to seed");
        return Ok(());
    }

    // Group all paired turns by session, restricted to affected sessions.
    // Lone user turns (no assistant response) are skipped — they don't
    // correspond to a completed API request.
    let mut new_rows_by_session: HashMap<String, Vec<Turn>> = HashMap::new();
    for t in all_turns.into_iter().filter(|t| t.te.is_some() && affected.contains(&t.sid)) {
        new_rows_by_session.entry(t.sid.clone()).or_default().push(t);
    }
    let new_total: usize = new_rows_by_session.values().map(|v| v.len()).sum();

    let lpath = ledger_path();
    let raw_existing = read_raw_lines(&lpath)?;
    let to_drop: usize = raw_existing
        .iter()
        .filter(|line| {
            sid_of_raw(line)
                .map(|s| affected.contains(&s))
                .unwrap_or(false)
        })
        .count();

    println!();
    println!("plan:");
    println!("  drop  {} existing ledger rows from {} affected sessions", to_drop, affected.len());
    println!("  write {} fresh rows from JSONL", new_total);
    println!("  preserve {} unrelated rows untouched", raw_existing.len() - to_drop);

    if args.dry_run {
        println!();
        println!("--dry-run; no changes written");
        return Ok(());
    }

    if to_drop == 0 && new_total == 0 {
        println!();
        println!("nothing to do");
        return Ok(());
    }

    // Backup
    let bak = lpath.with_extension(format!("jsonl.bak.{}", now_unix()));
    fs::copy(&lpath, &bak)?;
    println!();
    println!("backed up original to {}", bak.display());

    // Build new ledger: keep unrelated rows verbatim, plus newly synthesized
    // rows for affected sessions, then sort all by ts.
    let mut out_lines: Vec<String> = Vec::with_capacity(raw_existing.len() - to_drop + new_total);
    for raw in raw_existing {
        let drop = sid_of_raw(&raw)
            .map(|s| affected.contains(&s))
            .unwrap_or(false);
        if !drop {
            out_lines.push(raw);
        }
    }
    for (_sid, turns) in new_rows_by_session {
        for t in turns {
            out_lines.push(synthesize_record_json(&t));
        }
    }

    out_lines.sort_by(|a, b| {
        let ta = ts_of_raw(a).unwrap_or(0.0);
        let tb = ts_of_raw(b).unwrap_or(0.0);
        ta.partial_cmp(&tb).unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut out = fs::File::create(&lpath)?;
    for line in &out_lines {
        out.write_all(line.as_bytes())?;
        out.write_all(b"\n")?;
    }
    out.flush()?;

    println!();
    println!("✓ wrote {} total rows to {}", out_lines.len(), lpath.display());
    println!("  (backup retained at {})", bak.display());
    Ok(())
}

fn read_raw_lines(p: &Path) -> std::io::Result<Vec<String>> {
    let f = fs::File::open(p)?;
    let reader = BufReader::new(f);
    let mut out = Vec::new();
    for line in reader.lines().map_while(Result::ok) {
        let l = line.trim().to_string();
        if !l.is_empty() {
            out.push(l);
        }
    }
    Ok(out)
}

fn sid_of_raw(raw: &str) -> Option<String> {
    let v: Value = serde_json::from_str(raw).ok()?;
    v.get("sid").and_then(|s| s.as_str()).map(str::to_string)
}

fn ts_of_raw(raw: &str) -> Option<f64> {
    let v: Value = serde_json::from_str(raw).ok()?;
    v.get("ts").and_then(|t| t.as_f64())
}

/// Walk every JSONL under `dir`, pair each user event with the next
/// assistant event in the same session, emit one Turn per pair (plus
/// trailing lone user events with te=None, which the caller filters out).
fn collect_all_turns(
    dir: &Path,
    only_sid: Option<&str>,
) -> Result<Vec<Turn>, Box<dyn std::error::Error>> {
    let mut all_turns: Vec<Turn> = Vec::new();
    walk_jsonl(dir, &mut |p| {
        let Ok(f) = fs::File::open(p) else { return };
        let reader = BufReader::new(f);
        let mut pending_user: Option<Turn> = None;
        for line in reader.lines().map_while(Result::ok) {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
            let kind = v.get("type").and_then(|t| t.as_str()).unwrap_or("");
            let sid = v.get("sessionId").and_then(|s| s.as_str()).unwrap_or("");
            if sid.is_empty() {
                continue;
            }
            if let Some(want) = only_sid {
                if sid != want {
                    continue;
                }
            }
            let ts = v
                .get("timestamp")
                .and_then(|t| t.as_str())
                .and_then(parse_iso8601);
            let Some(ts) = ts else { continue };

            match kind {
                "user" => {
                    if let Some(t) = pending_user.take() {
                        all_turns.push(t);
                    }
                    let content = v.get("message").and_then(|m| m.get("content"));
                    let (u_ch, tr_ch) = count_user_chars(content);
                    pending_user = Some(Turn {
                        sid: sid.to_string(),
                        ts,
                        te: None,
                        u_ch, tr_ch,
                        model: None,
                        in_tok: 0, out_tok: 0, cr: 0, cc: 0,
                    });
                }
                "assistant" => {
                    let Some(mut t) = pending_user.take() else { continue };
                    let msg = v.get("message");
                    let usage = msg.and_then(|m| m.get("usage"));
                    t.te = Some(ts);
                    t.model = msg
                        .and_then(|m| m.get("model"))
                        .and_then(|m| m.as_str())
                        .map(str::to_string);
                    t.in_tok = usage.and_then(|u| u.get("input_tokens")).and_then(Value::as_u64).unwrap_or(0);
                    t.out_tok = usage.and_then(|u| u.get("output_tokens")).and_then(Value::as_u64).unwrap_or(0);
                    t.cr = usage.and_then(|u| u.get("cache_read_input_tokens")).and_then(Value::as_u64).unwrap_or(0);
                    t.cc = usage.and_then(|u| u.get("cache_creation_input_tokens")).and_then(Value::as_u64).unwrap_or(0);
                    all_turns.push(t);
                }
                _ => {}
            }
        }
        if let Some(t) = pending_user.take() {
            all_turns.push(t);
        }
    });
    Ok(all_turns)
}

fn walk_jsonl(dir: &Path, f: &mut dyn FnMut(&Path)) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            walk_jsonl(&p, f);
        } else if p.extension().and_then(|e| e.to_str()) == Some("jsonl") {
            f(&p);
        }
    }
}

/// Mirror of `handler.rs::extract_user_message_chars`. The JSONL stores the
/// same message structure as the API request body; we re-implement here
/// rather than share to keep the live request hot path free of any shared
/// parsing module.
fn count_user_chars(content: Option<&Value>) -> (u64, u64) {
    let Some(c) = content else { return (0, 0) };
    if let Some(s) = c.as_str() {
        return (s.chars().count() as u64, 0);
    }
    let Some(blocks) = c.as_array() else { return (0, 0) };
    let mut text = 0u64;
    let mut tool = 0u64;
    for b in blocks {
        let kind = b.get("type").and_then(|t| t.as_str()).unwrap_or("");
        match kind {
            "text" => {
                if let Some(t) = b.get("text").and_then(|t| t.as_str()) {
                    text += t.chars().count() as u64;
                }
            }
            "tool_result" => {
                if let Some(c) = b.get("content") {
                    if let Some(s) = c.as_str() {
                        tool += s.chars().count() as u64;
                    } else if let Some(arr) = c.as_array() {
                        for inner in arr {
                            if let Some(t) = inner.get("text").and_then(|t| t.as_str()) {
                                tool += t.chars().count() as u64;
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
    (text, tool)
}

fn synthesize_record_json(t: &Turn) -> String {
    let te = t.te.unwrap_or(t.ts);
    let lat_ms = ((te - t.ts) * 1000.0).max(0.0) as u64;
    let dt = format_local_dt(t.ts);
    json!({
        "ts": t.ts,
        "te": te,
        "dt": dt,
        "human": std::env::var("USER").unwrap_or_else(|_| "unknown".into()),
        "agent": "seed",
        "sid": t.sid,
        "cip": null,
        "pip": null,
        "sip": null,
        "ep": "https://api.anthropic.com/v1/messages",
        "reg": null,
        "model": t.model.as_deref().unwrap_or("unknown"),
        "in": t.in_tok,
        "out": t.out_tok,
        "tot": t.in_tok + t.out_tok,
        "lat": lat_ms,
        "cr": t.cr,
        "cc": t.cc,
        "c_us": 0,
        "u_ch": t.u_ch,
        "tr_ch": t.tr_ch,
    }).to_string()
}

fn parse_iso8601(s: &str) -> Option<f64> {
    let dt = time::OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339).ok()?;
    Some(dt.unix_timestamp() as f64 + dt.nanosecond() as f64 / 1e9)
}

fn parse_when(s: &str) -> Result<f64, Box<dyn std::error::Error>> {
    if let Ok(n) = s.parse::<f64>() {
        return Ok(n);
    }
    let fmt = time::format_description::parse("[year]-[month]-[day]")
        .map_err(|e| -> Box<dyn std::error::Error> { format!("bad format desc: {}", e).into() })?;
    let date = time::Date::parse(s, &fmt)
        .map_err(|e| -> Box<dyn std::error::Error> { format!("bad date {s}: {e}").into() })?;
    let local = time::UtcOffset::current_local_offset().unwrap_or(time::UtcOffset::UTC);
    let dt = time::PrimitiveDateTime::new(date, time::Time::MIDNIGHT).assume_offset(local);
    Ok(dt.unix_timestamp() as f64)
}

fn format_local_dt(ts: f64) -> String {
    let secs = ts as i64;
    let dt = time::OffsetDateTime::from_unix_timestamp(secs)
        .unwrap_or(time::OffsetDateTime::UNIX_EPOCH);
    let local = time::UtcOffset::current_local_offset().unwrap_or(time::UtcOffset::UTC);
    let dt = dt.to_offset(local);
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        dt.year(), u8::from(dt.month()), dt.day(),
        dt.hour(), dt.minute(), dt.second()
    )
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
