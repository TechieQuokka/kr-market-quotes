use crate::api::Row;
use anyhow::{Context, Result};
use chrono::{Datelike, FixedOffset, NaiveDate, TimeZone, Timelike, Utc};
use std::collections::HashMap;

pub fn kst() -> FixedOffset {
    FixedOffset::east_opt(9 * 3600).unwrap()
}

pub fn fmt_kst(ts: i64) -> String {
    Utc.timestamp_opt(ts, 0)
        .single()
        .map(|t| t.with_timezone(&kst()).format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|| ts.to_string())
}

/// "YYYY-MM-DD" → KST 자정(start) 또는 23:59:59(end)의 epoch초
pub fn parse_date(s: &str, end_of_day: bool) -> Result<i64> {
    let d = NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .with_context(|| format!("날짜 형식은 YYYY-MM-DD 입니다: {s}"))?;
    let t = if end_of_day {
        d.and_hms_opt(23, 59, 59).unwrap()
    } else {
        d.and_hms_opt(0, 0, 0).unwrap()
    };
    Ok(kst().from_local_datetime(&t).single().unwrap().timestamp())
}

/// "YYYY-MM" → 그 달 전체의 KST epoch초 구간
pub fn month_range(s: &str) -> Result<(i64, i64)> {
    let first = NaiveDate::parse_from_str(&format!("{s}-01"), "%Y-%m-%d")
        .with_context(|| format!("월 형식은 YYYY-MM 입니다: {s}"))?;
    let next = if first.month() == 12 {
        NaiveDate::from_ymd_opt(first.year() + 1, 1, 1)
    } else {
        NaiveDate::from_ymd_opt(first.year(), first.month() + 1, 1)
    }
    .unwrap();
    let start = kst()
        .from_local_datetime(&first.and_hms_opt(0, 0, 0).unwrap())
        .single()
        .unwrap()
        .timestamp();
    let end = kst()
        .from_local_datetime(&next.and_hms_opt(0, 0, 0).unwrap())
        .single()
        .unwrap()
        .timestamp()
        - 1;
    Ok((start, end))
}

pub struct Enriched {
    pub row: Row,
    pub delta: Option<f64>,
    pub delta_pct: Option<f64>,
}

/// 직전 수집분 대비 Δ/Δ%. 입력은 (symbol, ts) 정렬 상태를 가정 (API/캐시가 보장).
pub fn add_deltas(rows: &[Row]) -> Vec<Enriched> {
    let mut last: HashMap<String, f64> = HashMap::new();
    rows.iter()
        .map(|r| {
            let prev = last.insert(r.symbol.clone(), r.value);
            Enriched {
                row: r.clone(),
                delta: prev.map(|p| r.value - p),
                delta_pct: prev.map(|p| (r.value - p) / p * 100.0),
            }
        })
        .collect()
}

pub struct Trend {
    pub n: usize,
    pub first: f64,
    pub last: f64,
    pub high: f64,
    pub low: f64,
    pub change: f64,
    pub change_pct: f64,
    pub slope_per_day: f64, // 선형회귀 기울기 (하루당 변화량)
    pub verdict: &'static str,
}

pub fn trend(rows: &[Row]) -> Option<Trend> {
    if rows.len() < 2 {
        return None;
    }
    let first = rows.first().unwrap().value;
    let last = rows.last().unwrap().value;
    let high = rows.iter().map(|r| r.value).fold(f64::MIN, f64::max);
    let low = rows.iter().map(|r| r.value).fold(f64::MAX, f64::min);
    let change = last - first;
    let change_pct = change / first * 100.0;

    let t0 = rows[0].ts;
    let n = rows.len() as f64;
    let xs = rows.iter().map(|r| (r.ts - t0) as f64 / 86400.0);
    let mean_x = xs.clone().sum::<f64>() / n;
    let mean_y = rows.iter().map(|r| r.value).sum::<f64>() / n;
    let cov: f64 = xs
        .clone()
        .zip(rows.iter())
        .map(|(x, r)| (x - mean_x) * (r.value - mean_y))
        .sum();
    let var: f64 = xs.map(|x| (x - mean_x) * (x - mean_x)).sum();
    let slope_per_day = if var > 0.0 { cov / var } else { 0.0 };

    let verdict = if change_pct >= 0.5 {
        "상향"
    } else if change_pct <= -0.5 {
        "하향"
    } else {
        "보합"
    };

    Some(Trend {
        n: rows.len(),
        first,
        last,
        high,
        low,
        change,
        change_pct,
        slope_per_day,
        verdict,
    })
}

/// KST 매시 정각 ts에 해당 심볼 수집이 일어났어야 하는가 — worker의 targets()와 동일 규칙
pub fn expected(symbol: &str, ts: i64) -> bool {
    let k = Utc.timestamp_opt(ts, 0).single().unwrap().with_timezone(&kst());
    let day = k.weekday().number_from_monday(); // 1=월 … 7=일
    let hour = k.hour();
    match symbol {
        "KOSPI" | "KOSDAQ" => (1..=5).contains(&day) && (9..=16).contains(&hour),
        "USDKRW" => {
            ((1..=5).contains(&day) && !(day == 1 && hour < 6)) || (day == 6 && hour < 6)
        }
        _ => false,
    }
}

/// 정각 직후 이 초까지는 아직 같은 슬롯으로 본다.
///
/// 슬롯 T의 수집분은 대개 ts ∈ (T-1h, T]에 찍히지만(localTradedAt이 수집 시각보다 이르다),
/// 09:00 개장 틱만은 예외로 T보다 몇 초~수십 초 *늦게* 찍힌다. 네이버가 개장 시각을
/// 그 시점에 새로 스탬프하기 때문. 그냥 올림하면 이 틱이 10:00 슬롯으로 밀려서
/// 09:00은 결측으로, 10:00은 이중으로 잡힌다.
const SLOT_GRACE: i64 = 120;

/// ts가 속한 수집 슬롯 (KST 정각).
pub fn slot_of(ts: i64) -> i64 {
    (ts - SLOT_GRACE + 3599) / 3600 * 3600
}

/// 수집이 멈춘 것으로 볼 시간 (수집 시간대 안에서만 적용).
const STALE_HOURS: f64 = 2.0;

/// 수집이 멈췄는가.
///
/// 기준은 `last_run`(collect_log의 마지막 non-failed 실행)이지 `last_ts`(quotes의 마지막 행)가
/// 아니다. 값이 직전과 같으면 행을 쓰지 않으므로 행이 오래된 것과 수집이 멈춘 것은 다른
/// 사건이다. `last_run`이 없는 경우(구버전 worker · 로그 도입 전)만 quotes로 폴백한다.
pub fn is_stale(last_run: Option<i64>, last_ts: Option<i64>, in_window: bool, now: i64) -> bool {
    let too_old = |t: i64| in_window && (now - t) as f64 / 3600.0 > STALE_HOURS;
    match last_run {
        Some(t) => too_old(t),
        None => last_ts.is_none_or(too_old),
    }
}

/// [from, to] 구간에서 수집됐어야 하는데 빠진 정각 슬롯들.
pub fn missing_slots(symbol: &str, rows: &[Row], from: i64, to: i64) -> Vec<i64> {
    let filled: std::collections::HashSet<i64> = rows
        .iter()
        .filter(|r| r.symbol == symbol)
        .map(|r| slot_of(r.ts))
        .collect();
    let mut t = (from + 3599) / 3600 * 3600;
    let mut missing = Vec::new();
    while t <= to {
        if expected(symbol, t) && !filled.contains(&t) {
            missing.push(t);
        }
        t += 3600;
    }
    missing
}

#[cfg(test)]
mod tests {
    use super::*;

    /// KST "YYYY-MM-DD HH:MM:SS" → epoch초
    fn kts(s: &str) -> i64 {
        let n = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S").unwrap();
        kst().from_local_datetime(&n).single().unwrap().timestamp()
    }

    #[test]
    fn open_tick_stays_in_0900_slot() {
        // 실측: 개장 틱은 09:00 정각보다 몇 초~수십 초 늦게 찍힌다.
        for t in ["09:00:00", "09:00:05", "09:00:11", "09:00:28", "09:01:59"] {
            let ts = kts(&format!("2026-08-04 {t}"));
            assert_eq!(
                slot_of(ts),
                kts("2026-08-04 09:00:00"),
                "{t} 는 09:00 슬롯이어야 한다"
            );
        }
    }

    #[test]
    fn normal_tick_rounds_up_to_next_slot() {
        // 나머지 시각대는 localTradedAt이 수집 시각보다 이르다 → 올림.
        assert_eq!(slot_of(kts("2026-08-04 09:59:03")), kts("2026-08-04 10:00:00"));
        assert_eq!(slot_of(kts("2026-08-04 14:58:00")), kts("2026-08-04 15:00:00"));
        assert_eq!(slot_of(kts("2026-08-04 12:00:00")), kts("2026-08-04 12:00:00"));
        // USDKRW 한산한 시간대는 한참 뒤처진 ts가 온다 (05:28 → 06:00 수집분).
        assert_eq!(slot_of(kts("2026-07-22 05:28:12")), kts("2026-07-22 06:00:00"));
    }

    #[test]
    fn open_tick_does_not_collide_with_1000_slot() {
        // 회귀 방지: 09:00:11 틱이 10:00 슬롯으로 밀리면
        // 09:00은 결측, 10:00은 이중으로 잡혔다.
        let open = kts("2026-08-04 09:00:11");
        let ten = kts("2026-08-04 09:59:03");
        assert_ne!(slot_of(open), slot_of(ten));
    }

    // ── 신선도 판정 ──────────────────────────────────────────────
    // 핵심은 두 방향을 다 지키는 것이다: 값이 안 변해도 경보가 울리면 안 되고(오탐),
    // 수집이 실제로 멈추면 반드시 울려야 한다(미탐). 후자가 이 명령의 존재 이유다.

    #[test]
    fn unchanged_value_during_session_is_not_stale() {
        // USDKRW 새벽 한산 시간대 · 개장 09시대: 수집은 매시 돌지만 값이 그대로라
        // quotes에는 행이 안 쌓인다. 이걸 경보로 잡던 것이 제거한 오탐이다.
        let now = kts("2026-08-06 14:00:00");
        let last_run = Some(kts("2026-08-06 14:00:02"));
        let last_ts = Some(kts("2026-08-06 04:59:00")); // 9시간 전 행
        assert!(!is_stale(last_run, last_ts, true, now));
    }

    #[test]
    fn collection_stopped_during_session_is_stale() {
        // 미탐 방지 — 수집이 실제로 멈춘 경우. 2026-08-07 실제 장애를 옮긴 것:
        // 크론이 08-07 02:00~18:00 KST 동안 안 돌아 금요일 장 전체가 비었다.
        let now = kts("2026-08-07 14:00:00");
        let last_run = Some(kts("2026-08-06 16:00:02")); // 22시간 전
        let last_ts = Some(kts("2026-08-06 15:59:00"));
        assert!(is_stale(last_run, last_ts, true, now));
    }

    #[test]
    fn all_runs_failed_is_stale() {
        // 전부 실패하면 non-failed 실행이 없어 last_run 자체가 안 잡힌다.
        // quotes도 안 쌓이므로 폴백 경로에서 걸려야 한다.
        let now = kts("2026-08-07 14:00:00");
        assert!(is_stale(None, Some(kts("2026-08-06 15:59:00")), true, now));
        assert!(is_stale(None, None, true, now)); // 7일 내 데이터 자체가 없음
    }

    #[test]
    fn legacy_worker_falls_back_to_quotes() {
        // 구버전 worker (collect-log 없음) → quotes 기준 예전 판정 유지.
        let now = kts("2026-08-06 14:00:00");
        assert!(!is_stale(None, Some(kts("2026-08-06 13:59:00")), true, now));
        assert!(is_stale(None, Some(kts("2026-08-06 10:00:00")), true, now));
    }

    #[test]
    fn outside_collection_window_is_never_stale() {
        // 주말·장 마감 후에는 아무리 오래돼도 경보가 아니다.
        let now = kts("2026-08-08 16:00:00"); // 토요일
        assert!(!is_stale(Some(kts("2026-08-06 16:00:02")), None, false, now));
    }

    #[test]
    fn missing_slots_reports_nothing_for_a_full_trading_day() {
        let day = "2026-08-04";
        // 09:00 개장 틱 + 10:00~16:00 수집분(각 1분 이르게)
        let mut rows = vec![Row {
            symbol: "KOSPI".into(),
            ts: kts(&format!("{day} 09:00:11")),
            value: 1.0,
        }];
        for h in 9..16 {
            rows.push(Row {
                symbol: "KOSPI".into(),
                ts: kts(&format!("{day} {h:02}:59:03")),
                value: 1.0,
            });
        }
        let from = kts(&format!("{day} 00:00:00"));
        let to = kts(&format!("{day} 23:59:59"));
        assert_eq!(missing_slots("KOSPI", &rows, from, to), Vec::<i64>::new());
    }
}
