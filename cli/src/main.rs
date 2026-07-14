mod api;
mod cache;
mod commands;
mod compute;
mod config;
mod output;

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "kmq",
    version,
    about = "코스피/코스닥/환율 수집 데이터 조회 CLI",
    disable_help_subcommand = true
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// 구간 시세 조회 (직전 대비 Δ/Δ% 포함)
    Fetch {
        /// KOSPI | KOSDAQ | USDKRW
        #[arg(long)]
        symbol: String,
        /// YYYY-MM-DD (KST)
        #[arg(long)]
        from: String,
        /// YYYY-MM-DD (생략 시 현재까지)
        #[arg(long)]
        to: Option<String>,
        /// JSON으로 출력
        #[arg(long)]
        json: bool,
    },
    /// 3종목 최신값
    Latest,
    /// 구간 추세 판정 (상향/하향/보합)
    Trend {
        /// KOSPI | KOSDAQ | USDKRW
        #[arg(long)]
        symbol: String,
        /// YYYY-MM (예: 2026-07)
        #[arg(long)]
        month: Option<String>,
        /// YYYY-MM-DD
        #[arg(long)]
        from: Option<String>,
        /// YYYY-MM-DD
        #[arg(long)]
        to: Option<String>,
    },
    /// csv/jsonl 내보내기 (인자 없이 실행하면 전체 백업)
    Export {
        /// KOSPI | KOSDAQ | USDKRW (생략 시 전체)
        #[arg(long)]
        symbol: Option<String>,
        /// YYYY-MM-DD (생략 시 처음부터)
        #[arg(long)]
        from: Option<String>,
        /// YYYY-MM-DD (생략 시 현재까지)
        #[arg(long)]
        to: Option<String>,
        #[arg(long, value_parser = ["csv", "jsonl"], default_value = "jsonl")]
        format: String,
        /// 생략 시 stdout
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// 수집 상태 점검 (최종 수집 시각 + 최근 7일 빈 슬롯)
    Status,
    /// 로컬 캐시 관리
    Cache {
        #[command(subcommand)]
        action: CacheCmd,
    },
    /// 도움말 (--detail: 기능별 사용 예시 모음)
    Help {
        /// 각 기능의 사용 예시를 자세히 출력
        #[arg(long)]
        detail: bool,
    },
}

const DETAIL_HELP: &str = concat!(
    "kmq ",
    env!("CARGO_PKG_VERSION"),
    r#" — 기능별 사용 예시 모음
=====================================

■ latest — 3종목(KOSPI/KOSDAQ/USDKRW) 최신값을 한눈에
    $ kmq latest

■ fetch — 구간 시세 조회 (직전 수집분 대비 Δ/Δ% 포함)
    $ kmq fetch --symbol KOSPI --from 2026-07-01
    $ kmq fetch --symbol KOSDAQ --from 2026-07-01 --to 2026-07-14
    $ kmq fetch --symbol usdkrw --from 2026-07-14 --json      # 소문자도 허용, JSON 출력

■ trend — 구간 추세 판정 (상향/하향/보합 + 변화율/고저/회귀 기울기)
    $ kmq trend --symbol KOSPI --month 2026-07                # 특정 월 전체
    $ kmq trend --symbol USDKRW --from 2026-07-01             # 날짜부터 현재까지
    $ kmq trend --symbol KOSDAQ --from 2026-01-01 --to 2026-06-30

■ export — 데이터 내보내기 (학습 파이프라인/백업 용도)
    $ kmq export                                              # 전체를 jsonl로 stdout에
    $ kmq export --out backup.jsonl                           # 전체 백업
    $ kmq export --symbol KOSPI --from 2026-07-01 --format csv --out kospi.csv
    $ kmq export --symbol USDKRW | jq '.value'                # 파이프 연결

■ status — 수집 상태 점검 (이상 시 exit code 1)
    $ kmq status
    → 심볼별 최종 수집 시각 + 최근 7일 빈 슬롯 수.
      ⚠ 가 뜨면 네이버 API 변경/장애 의심. worker 로그(wrangler tail) 확인.
      공휴일이나 값 미변동(중복 제거)도 빈 슬롯으로 집계될 수 있음.

■ cache — 로컬 캐시 관리 (~/.cache/kmq/cache.db)
    $ kmq cache info                                          # 심볼별 저장 현황
    $ kmq cache clear                                         # 전체 삭제 (다시 받으면 됨)

설정 파일: ~/.config/kmq/config.toml (api_url, api_key)
날짜는 모두 KST 기준, 형식은 YYYY-MM-DD / 월은 YYYY-MM.
"#
);

#[derive(Subcommand)]
enum CacheCmd {
    /// 캐시 내용 요약
    Info,
    /// 캐시 전체 삭제
    Clear,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Fetch { symbol, from, to, json } => {
            commands::fetch::run(&symbol, &from, to.as_deref(), json)
        }
        Cmd::Latest => commands::latest::run(),
        Cmd::Trend { symbol, month, from, to } => {
            commands::trend::run(&symbol, month.as_deref(), from.as_deref(), to.as_deref())
        }
        Cmd::Export { symbol, from, to, format, out } => {
            commands::export::run(symbol.as_deref(), from.as_deref(), to.as_deref(), &format, out.as_deref())
        }
        Cmd::Status => commands::status::run(),
        Cmd::Cache { action } => match action {
            CacheCmd::Info => {
                let c = cache::Cache::open()?;
                println!("경로: {}", cache::cache_path()?.display());
                let info = c.info()?;
                if info.is_empty() {
                    println!("(비어 있음)");
                } else {
                    let rows: Vec<Vec<String>> = info
                        .iter()
                        .map(|(s, n, min, max)| {
                            vec![
                                s.clone(),
                                n.to_string(),
                                compute::fmt_kst(*min),
                                compute::fmt_kst(*max),
                            ]
                        })
                        .collect();
                    output::print_table(&["symbol", "rows", "from(KST)", "to(KST)"], &rows);
                }
                Ok(())
            }
            CacheCmd::Clear => {
                cache::clear()?;
                println!("캐시 삭제 완료");
                Ok(())
            }
        },
        Cmd::Help { detail } => {
            if detail {
                print!("{DETAIL_HELP}");
            } else {
                use clap::CommandFactory;
                // 파이프가 먼저 닫혀도(broken pipe) 조용히 종료
                let _ = Cli::command().print_help();
            }
            Ok(())
        }
    }
}
