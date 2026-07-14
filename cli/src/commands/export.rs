use crate::{api, cache, compute, config, output};
use anyhow::Result;
use std::io::Write;
use std::path::Path;

pub fn run(
    symbol: Option<&str>,
    from: Option<&str>,
    to: Option<&str>,
    format: &str,
    out: Option<&Path>,
) -> Result<()> {
    let cfg = config::load()?;
    let mut c = cache::Cache::open()?;

    // from 생략 시 전체(0부터) — 전체 백업 용도
    let from_ts = match from {
        Some(f) => compute::parse_date(f, false)?,
        None => 0,
    };
    let to_ts = match to {
        Some(t) => compute::parse_date(t, true)?,
        None => chrono::Utc::now().timestamp(),
    };
    let symbols: Vec<String> = match symbol {
        Some(s) => vec![api::norm_symbol(s)?],
        None => api::SYMBOLS.iter().map(|s| s.to_string()).collect(),
    };

    let mut all = Vec::new();
    for s in &symbols {
        all.extend(cache::get_range(&cfg, &mut c, s, from_ts, to_ts)?);
    }
    let enriched = compute::add_deltas(&all);

    let mut w: Box<dyn Write> = match out {
        Some(p) => Box::new(std::fs::File::create(p)?),
        None => Box::new(std::io::stdout()),
    };
    match format {
        "csv" => output::write_csv(&mut w, &enriched)?,
        _ => output::write_jsonl(&mut w, &enriched)?,
    }
    w.flush()?;
    if let Some(p) = out {
        eprintln!("{}건 → {}", enriched.len(), p.display());
    }
    Ok(())
}
