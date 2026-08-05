use crate::{api, compute, config, output};
use anyhow::Result;

/// 수집 상태 점검 — 항상 API 직행 (캐시를 거치면 상태 점검의 의미가 없음)
pub fn run() -> Result<()> {
    let cfg = config::load()?;
    let now = chrono::Utc::now().timestamp();
    let from = now - 7 * 86400;
    let rows = api::fetch_quotes(&cfg, None, from, now)?;
    let log_opt = api::fetch_collect_log(&cfg, from, now)?;
    let has_endpoint = log_opt.is_some();
    let log = log_opt.unwrap_or_default();

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

        // 수집 로그가 있으면 빈 슬롯을 '실패'와 '값 미변동'으로 갈라볼 수 있다.
        let fails = log.iter().filter(|l| l.symbol == sym && l.outcome == "failed").count();
        // 로그에 성공/미변동으로 남은 슬롯은 결측이 아니다 — 수집은 정상적으로 돌았다.
        let logged_ok: std::collections::HashSet<i64> = log
            .iter()
            .filter(|l| l.symbol == sym && l.outcome != "failed")
            .map(|l| compute::slot_of(l.run_ts))
            .collect();
        let unexplained = missing.iter().filter(|t| !logged_ok.contains(t)).count();

        let flag = if stale || fails > 0 || unexplained > 5 {
            warn = true;
            "⚠"
        } else {
            "OK"
        };
        let missing_s = if last_ts.is_some() { unexplained.to_string() } else { "-".into() };
        let fails_s = if has_endpoint { fails.to_string() } else { "?".into() };
        table.push(vec![sym.to_string(), last_s, missing_s, fails_s, flag.to_string()]);
    }

    output::print_table(
        &["symbol", "last collected", "unexplained(7d)", "failed(7d)", "state"],
        &table,
    );
    if !has_endpoint {
        println!(
            "\n⚠ worker에 /collect-log가 없습니다 (구버전). 실패와 값 미변동을 구분할 수 없어\n\
             unexplained 수치에 공휴일·중복 제거분이 섞여 있습니다. worker를 재배포하세요."
        );
    } else if log.is_empty() {
        println!(
            "\n※ 수집 로그가 아직 비어 있습니다 (worker 배포 직후). 다음 정각 수집부터 쌓이며,\n\
             그 전까지 unexplained에는 공휴일·중복 제거분이 섞여 있습니다."
        );
    } else {
        println!("\nunexplained = 수집 로그로 설명되지 않는 빈 슬롯 (실패도 미변동도 아닌 것).");
    }
    if warn {
        println!("⚠ 수집 이상 의심 — `wrangler tail`로 worker 로그와 네이버 응답을 확인하세요.");
        std::process::exit(1);
    }
    Ok(())
}
