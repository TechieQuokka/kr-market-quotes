use crate::{api, compute, config, output};
use anyhow::Result;

/// 수집 상태 점검 — 항상 API 직행 (캐시를 거치면 상태 점검의 의미가 없음)
pub fn run() -> Result<()> {
    let cfg = config::load()?;
    let now = chrono::Utc::now().timestamp();
    let from = now - 7 * 86400;
    let rows = api::fetch_quotes(&cfg, None, from, now)?;

    let mut table = Vec::new();
    let mut warn = false;
    for sym in api::SYMBOLS {
        let sym_ts = || rows.iter().filter(|r| r.symbol == sym).map(|r| r.ts);
        let last_ts = sym_ts().max();
        // 수집 이력이 창 중간에 시작됐으면 그 이후만 검사 (도입 첫 주 노이즈 방지)
        let from_eff = sym_ts().min().map_or(from, |first| from.max(first));
        let missing = compute::missing_slots(sym, &rows, from_eff, now);

        let (last_s, stale) = match last_ts {
            Some(t) => {
                let age_h = (now - t) as f64 / 3600.0;
                // 지금이 수집 시간대인데 2시간 넘게 새 데이터가 없으면 경고
                let stale = compute::expected(sym, now / 3600 * 3600) && age_h > 2.0;
                (format!("{} ({:.0}h 전)", compute::fmt_kst(t), age_h), stale)
            }
            None => ("no data (7d)".to_string(), true),
        };

        let flag = if stale || missing.len() > 5 {
            warn = true;
            "⚠"
        } else {
            "OK"
        };
        let missing_s = if last_ts.is_some() { missing.len().to_string() } else { "-".into() };
        table.push(vec![sym.to_string(), last_s, missing_s, flag.to_string()]);
    }

    output::print_table(&["symbol", "last collected", "missing(7d)", "state"], &table);
    println!("\n참고: 공휴일·값 미변동(중복 제거분)도 빈 슬롯으로 집계될 수 있습니다.");
    if warn {
        println!("⚠ 수집 이상 의심 — `wrangler tail`로 worker 로그와 네이버 응답을 확인하세요.");
        std::process::exit(1);
    }
    Ok(())
}
