use crate::{api, cache, compute, config};
use anyhow::{Result, bail};

pub fn run(symbol: &str, month: Option<&str>, from: Option<&str>, to: Option<&str>) -> Result<()> {
    let symbol = api::norm_symbol(symbol)?;

    let (from_ts, to_ts, label) = match (month, from) {
        (Some(m), _) => {
            let (f, t) = compute::month_range(m)?;
            (f, t, m.to_string())
        }
        (None, Some(f)) => {
            let ft = compute::parse_date(f, false)?;
            let tt = match to {
                Some(t) => compute::parse_date(t, true)?,
                None => chrono::Utc::now().timestamp(),
            };
            (ft, tt, format!("{f} ~ {}", to.unwrap_or("현재")))
        }
        _ => bail!("--month YYYY-MM 또는 --from YYYY-MM-DD 를 지정하세요"),
    };

    let cfg = config::load()?;
    let mut c = cache::Cache::open()?;
    let rows = cache::get_range(&cfg, &mut c, &symbol, from_ts, to_ts)?;

    match compute::trend(&rows) {
        None => println!("{symbol} {label}: 데이터 부족 ({}건)", rows.len()),
        Some(t) => {
            println!("{symbol} {label} 추세: {} ({:+.2}%)", t.verdict, t.change_pct);
            println!("  처음 {:.2} → 마지막 {:.2} ({:+.2})", t.first, t.last, t.change);
            println!("  최고 {:.2} / 최저 {:.2}", t.high, t.low);
            println!("  회귀 기울기 {:+.2}/일 · 표본 {}건", t.slope_per_day, t.n);
        }
    }
    Ok(())
}
