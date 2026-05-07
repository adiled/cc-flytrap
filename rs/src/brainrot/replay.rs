//! Animated replay. Walks records chronologically with sleep gaps scaled by
//! --speed. --follow tails the live ledger after the replay.

use crate::brainrot::aggregate::short_model;
use crate::ledger_read::{iter_records, parse_range, Record};
use crate::theme::*;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::time::Duration;

pub fn run(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let mut speed: f64 = 1.0;
    let mut follow = false;
    let mut rest: Vec<String> = Vec::new();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--speed" | "-s" if i + 1 < args.len() => {
                speed = args[i + 1].parse().unwrap_or(1.0);
                i += 2;
            }
            "--follow" | "-f" => {
                follow = true;
                i += 1;
            }
            other => {
                rest.push(other.to_string());
                i += 1;
            }
        }
    }
    let spec = if rest.is_empty() {
        "24h".to_string()
    } else {
        rest.join(" ")
    };
    let range = parse_range(&spec).map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
    let records: Vec<Record> = iter_records(Some(range.since), Some(range.until)).collect();

    if records.is_empty() && !follow {
        println!("\n  {}\n", dim(&format!("no records in {}", range.label)));
        return Ok(());
    }

    header(
        "brainrot replay",
        &format!("{} · {} records · {}x", range.label, records.len(), speed),
    );

    let mut prev_ts = records.first().map(|r| r.ts).unwrap_or(0.0);
    let local = time::UtcOffset::current_local_offset().unwrap_or(time::UtcOffset::UTC);

    for r in &records {
        let gap = (r.ts - prev_ts).max(0.0);
        let sleep_s = if speed > 0.0 {
            (gap / speed).min(2.0)
        } else {
            0.0
        };
        if sleep_s > 0.01 {
            std::thread::sleep(Duration::from_secs_f64(sleep_s));
        }
        prev_ts = r.ts;
        print_record(r, local);
    }

    if follow {
        println!("\n  {}", grey("following live ledger… (ctrl-c to stop)"));
        follow_ledger(local)?;
    }
    Ok(())
}

fn print_record(r: &Record, local: time::UtcOffset) {
    let dt = time::OffsetDateTime::from_unix_timestamp(r.ts as i64)
        .unwrap_or(time::OffsetDateTime::UNIX_EPOCH)
        .to_offset(local);
    let when = format!("{:02}:{:02}:{:02}", dt.hour(), dt.minute(), dt.second());
    let model = short_model(r.model.as_deref().unwrap_or("?"));
    let model_tag = format!("[{:11}]", model);
    let lat_str = format!("{}ms", r.lat);
    let sid = r.sid.as_deref().unwrap_or("");
    let sid_short: String = sid.chars().take(8).collect();
    let marker = if r.lat > 5000 {
        yellow("⚠ slow")
    } else if r.lat < 500 {
        grey("· fast")
    } else {
        String::new()
    };
    println!(
        "  {}  {}  in:{:>6}  out:{:>6}  lat:{:>8}  {}  {}",
        dim(&when),
        cyan(&model_tag),
        fmt_n(r.r#in),
        fmt_n(r.out),
        heat_ms(r.lat as f64, &lat_str),
        dim(&format!("sid:{}", sid_short)),
        marker
    );
}

fn follow_ledger(local: time::UtcOffset) -> Result<(), Box<dyn std::error::Error>> {
    let path = crate::config::paths::ledger();
    let mut last_size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    loop {
        std::thread::sleep(Duration::from_millis(500));
        let size = match std::fs::metadata(&path) {
            Ok(m) => m.len(),
            Err(_) => continue,
        };
        if size <= last_size {
            continue;
        }
        let f = match std::fs::File::open(&path) {
            Ok(f) => f,
            Err(_) => continue,
        };
        let mut reader = BufReader::new(f);
        reader.seek(SeekFrom::Start(last_size))?;
        for line in reader.lines().map_while(Result::ok) {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let v: serde_json::Value = match serde_json::from_str(trimmed) {
                Ok(v) => v,
                Err(_) => continue,
            };
            if let Some(r) = Record::from_value(&v) {
                println!("  {} {}", green("●"), {
                    print_record(&r, local);
                    String::new()
                });
            }
        }
        last_size = size;
    }
}
