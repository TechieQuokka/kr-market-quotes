use crate::{api, cache, compute, config, output};
use anyhow::Result;

pub fn run(symbol: &str, from: &str, to: Option<&str>, json: bool) -> Result<()> {
    let symbol = api::norm_symbol(symbol)?;
    let cfg = config::load()?;
    let mut c = cache::Cache::open()?;

    let from_ts = compute::parse_date(from, false)?;
    let to_ts = match to {
        Some(t) => compute::parse_date(t, true)?,
        None => chrono::Utc::now().timestamp(),
    };

    let rows = cache::get_range(&cfg, &mut c, &symbol, from_ts, to_ts)?;
    let enriched = compute::add_deltas(&rows);

    if json {
        output::write_json(&mut std::io::stdout(), &enriched)?;
        return Ok(());
    }

    println!("{symbol} ({}건)", enriched.len());
    let table: Vec<Vec<String>> = enriched
        .iter()
        .map(|e| {
            vec![
                compute::fmt_kst(e.row.ts),
                format!("{:.2}", e.row.value),
                output::fmt_delta(e.delta),
                output::fmt_pct(e.delta_pct),
            ]
        })
        .collect();
    output::print_table(&["time(KST)", "value", "delta", "delta%"], &table);
    Ok(())
}
