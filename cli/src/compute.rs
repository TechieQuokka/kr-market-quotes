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

/// [from, to] 구간에서 수집됐어야 하는데 빠진 정각 슬롯들.
/// 슬롯 T의 수집분은 ts ∈ (T-1h, T]에 찍힌다 (네이버 localTradedAt이 수집 시각보다 약간 이르므로).
pub fn missing_slots(symbol: &str, rows: &[Row], from: i64, to: i64) -> Vec<i64> {
    let filled: std::collections::HashSet<i64> = rows
        .iter()
        .filter(|r| r.symbol == symbol)
        .map(|r| (r.ts + 3599) / 3600 * 3600) // ts가 속한 슬롯 T = 올림 정각
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
