use crate::config::Config;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

pub const SYMBOLS: [&str; 3] = ["KOSPI", "KOSDAQ", "USDKRW"];

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Row {
    pub symbol: String,
    pub ts: i64,
    pub value: f64,
}

pub fn norm_symbol(s: &str) -> Result<String> {
    let u = s.to_uppercase();
    if SYMBOLS.contains(&u.as_str()) {
        Ok(u)
    } else {
        bail!("지원하지 않는 심볼: {s} (지원: KOSPI, KOSDAQ, USDKRW)")
    }
}

pub fn fetch_quotes(cfg: &Config, symbol: Option<&str>, from: i64, to: i64) -> Result<Vec<Row>> {
    let mut url = format!("{}/quotes?from={from}&to={to}", cfg.api_url);
    if let Some(s) = symbol {
        url.push_str(&format!("&symbol={s}"));
    }
    let rows: Vec<Row> = ureq::get(&url)
        .header("Authorization", &format!("Bearer {}", cfg.api_key))
        .call()
        .context("API 요청 실패 (404라면 api_url/api_key 확인)")?
        .body_mut()
        .read_json()
        .context("API 응답 파싱 실패")?;
    Ok(rows)
}
