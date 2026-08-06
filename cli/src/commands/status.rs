use crate::{api, compute, config, output};
use anyhow::Result;

/// "08-05 15:59 (16h 전)" — 7일 창 안만 다루므로 연도는 지면 낭비다.
fn fmt_age(ts: i64, now: i64) -> String {
    let t = compute::fmt_kst(ts);
    let short = t.get(5..).unwrap_or(&t);
    format!("{short} ({:.0}h 전)", (now - ts) as f64 / 3600.0)
}

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

        // 수집 로그가 있으면 빈 슬롯을 '실패'와 '값 미변동'으로 갈라볼 수 있다.
        let fails = log.iter().filter(|l| l.symbol == sym && l.outcome == "failed").count();
        // 마지막으로 정상적으로 돌아간 수집. 신선도 판정의 기준은 이것이다 —
        // quotes의 마지막 행이 아니라. 값이 그대로면 행을 쓰지 않으므로, 행이 오래된 것과
        // 수집이 멈춘 것은 다른 사건이다 (새벽 한산한 시간대 USDKRW가 매일 그렇다).
        let last_run = log
            .iter()
            .filter(|l| l.symbol == sym && l.outcome != "failed")
            .map(|l| l.run_ts)
            .max();

        // 지금이 수집 시간대인데 2시간 넘게 정상 수집이 없으면 경고
        let in_window = compute::expected(sym, now / 3600 * 3600);
        let too_old = |t: i64| in_window && (now - t) as f64 / 3600.0 > 2.0;
        let (run_s, stale) = match last_run {
            Some(t) => (fmt_age(t, now), too_old(t)),
            // 로그가 없으면(구버전 worker · 배포 직후) 예전처럼 quotes로 판정할 수밖에 없다.
            None => ("-".to_string(), last_ts.is_none_or(too_old)),
        };
        let quote_s = match last_ts {
            Some(t) => fmt_age(t, now),
            None => "no data (7d)".to_string(),
        };
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
        table.push(vec![sym.to_string(), quote_s, run_s, missing_s, fails_s, flag.to_string()]);
    }

    output::print_table(
        &["symbol", "last quote", "last run", "unexplained(7d)", "failed(7d)", "state"],
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
        println!(
            "\nlast run  = 마지막으로 정상 수집이 돌아간 시각. 상태 판정은 이 값 기준이다.\n\
             last quote = 값이 마지막으로 바뀐 시각. 값이 그대로면 행을 쓰지 않으므로 오래돼도 정상.\n\
             unexplained = 수집 로그로 설명되지 않는 빈 슬롯 (실패도 미변동도 아닌 것)."
        );
    }
    if warn {
        println!("⚠ 수집 이상 의심 — `wrangler tail`로 worker 로그와 네이버 응답을 확인하세요.");
        std::process::exit(1);
    }
    Ok(())
}
