use crate::{api, compute, config, output};
use anyhow::Result;

/// 최신값은 항상 API에서 직접 (캐시 미사용 — 신선도가 목적이므로)
pub fn run() -> Result<()> {
    let cfg = config::load()?;
    let now = chrono::Utc::now().timestamp();
    let rows = api::fetch_quotes(&cfg, None, now - 7 * 86400, now)?;
    let enriched = compute::add_deltas(&rows);

    let mut table = Vec::new();
    for sym in api::SYMBOLS {
        match enriched.iter().filter(|e| e.row.symbol == sym).next_back() {
            Some(e) => table.push(vec![
                sym.to_string(),
                compute::fmt_kst(e.row.ts),
                format!("{:.2}", e.row.value),
                output::fmt_delta(e.delta),
                output::fmt_pct(e.delta_pct),
            ]),
            None => table.push(vec![
                sym.to_string(),
                "-".into(),
                "-".into(),
                "-".into(),
                "-".into(),
            ]),
        }
    }
    output::print_table(&["symbol", "time(KST)", "value", "delta", "delta%"], &table);
    Ok(())
}
