use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Deserialize)]
pub struct Config {
    pub api_url: String,
    pub api_key: String,
}

pub fn config_path() -> Result<PathBuf> {
    let home = std::env::var("HOME").context("HOME 환경변수가 없습니다")?;
    Ok(PathBuf::from(home).join(".config/kmq/config.toml"))
}

pub fn load() -> Result<Config> {
    let path = config_path()?;
    let text = std::fs::read_to_string(&path).with_context(|| {
        format!(
            "설정 파일이 없습니다: {}\napi_url, api_key 두 항목을 담은 TOML 파일을 만들어 주세요",
            path.display()
        )
    })?;
    let mut cfg: Config = toml::from_str(&text).context("설정 파일 파싱 실패")?;
    cfg.api_url = cfg.api_url.trim_end_matches('/').to_string();
    Ok(cfg)
}
