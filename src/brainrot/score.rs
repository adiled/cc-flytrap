//! One-line brainrot score. Good for status bars / cron pipes.

use crate::brainrot::aggregate::*;
use crate::ledger_read::{iter_records, parse_range};
use crate::theme::*;

pub fn run(spec: &str) -> Result<(), Box<dyn std::error::Error>> {
    let range = parse_range(spec).map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
    let a = Aggregate::ingest(iter_records(Some(range.since), Some(range.until)));
    let baseline_records: Vec<_> = iter_records(None, None).collect();
    let baseline = Baseline::from_records(&baseline_records);

    if a.n == 0 {
        println!("brainrot: no data in {}", range.label);
        return Ok(());
    }
    let bot = bot_score(&a, &baseline);
    let drv = driver_score(&a, &baseline);
    let bot_s = score_color(bot, &format!("bot {}", bot));
    let drv_s = score_color(drv, &format!("driver {}", drv));
    println!(
        "{} · {} · {} {}",
        cyan("brainrot"),
        bot_s,
        drv_s,
        grey(&format!("({})", range.label))
    );
    Ok(())
}

fn score_color(score: u32, s: &str) -> String {
    if score < 40 {
        green(s)
    } else if score < 70 {
        yellow(s)
    } else {
        red(s)
    }
}
