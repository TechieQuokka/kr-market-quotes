use crate::api::{self, Row};
use crate::config::Config;
use anyhow::{Context, Result};
use rusqlite::{Connection, params};
use std::path::PathBuf;

/// 과거 시세는 불변이므로, 한 번 받아온 구간은 로컬에 저장하고 다시 요청하지 않는다.
/// ranges 테이블이 "완전히 받아둔 구간"을 기록한다.
pub struct Cache {
    conn: Connection,
}

pub fn cache_path() -> Result<PathBuf> {
    let home = std::env::var("HOME").context("HOME 환경변수가 없습니다")?;
    Ok(PathBuf::from(home).join(".cache/kmq/cache.db"))
}

impl Cache {
    pub fn open() -> Result<Self> {
        let path = cache_path()?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let conn = Connection::open(&path)?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS quotes (
               symbol TEXT NOT NULL,
               ts     INTEGER NOT NULL,
               value  REAL NOT NULL,
               PRIMARY KEY (symbol, ts)
             ) WITHOUT ROWID;
             CREATE TABLE IF NOT EXISTS ranges (
               symbol  TEXT NOT NULL,
               from_ts INTEGER NOT NULL,
               to_ts   INTEGER NOT NULL
             );",
        )?;
        Ok(Self { conn })
    }

    fn covered(&self, symbol: &str, from: i64, to: i64) -> Result<bool> {
        let n: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM ranges WHERE symbol = ?1 AND from_ts <= ?2 AND to_ts >= ?3",
            params![symbol, from, to],
            |r| r.get(0),
        )?;
        Ok(n > 0)
    }

    fn get(&self, symbol: &str, from: i64, to: i64) -> Result<Vec<Row>> {
        let mut stmt = self.conn.prepare(
            "SELECT symbol, ts, value FROM quotes
             WHERE symbol = ?1 AND ts >= ?2 AND ts <= ?3 ORDER BY ts",
        )?;
        let rows = stmt
            .query_map(params![symbol, from, to], |r| {
                Ok(Row { symbol: r.get(0)?, ts: r.get(1)?, value: r.get(2)? })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    fn put(&mut self, symbol: &str, from: i64, to: i64, rows: &[Row]) -> Result<()> {
        let tx = self.conn.transaction()?;
        for row in rows {
            tx.execute(
                "INSERT OR IGNORE INTO quotes (symbol, ts, value) VALUES (?1, ?2, ?3)",
                params![row.symbol, row.ts, row.value],
            )?;
        }
        // 겹치거나 맞닿은 기존 구간을 흡수해 하나로 병합
        let (min_f, max_t): (Option<i64>, Option<i64>) = tx.query_row(
            "SELECT MIN(from_ts), MAX(to_ts) FROM ranges
             WHERE symbol = ?1 AND from_ts <= ?3 + 1 AND to_ts >= ?2 - 1",
            params![symbol, from, to],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        tx.execute(
            "DELETE FROM ranges WHERE symbol = ?1 AND from_ts <= ?3 + 1 AND to_ts >= ?2 - 1",
            params![symbol, from, to],
        )?;
        tx.execute(
            "INSERT INTO ranges (symbol, from_ts, to_ts) VALUES (?1, ?2, ?3)",
            params![symbol, min_f.map_or(from, |v| v.min(from)), max_t.map_or(to, |v| v.max(to))],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// symbol별 (행 수, 최소 ts, 최대 ts)
    pub fn info(&self) -> Result<Vec<(String, i64, i64, i64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT symbol, COUNT(*), MIN(ts), MAX(ts) FROM quotes GROUP BY symbol ORDER BY symbol",
        )?;
        let rows = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }
}

pub fn clear() -> Result<()> {
    let path = cache_path()?;
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    Ok(())
}

/// 캐시 우선 조회. 미커버 구간만 API로 받아오고, "지금-1시간"까지만 커버로 기록해
/// 아직 도착 전일 수 있는 최신 데이터를 놓치지 않는다.
pub fn get_range(cfg: &Config, cache: &mut Cache, symbol: &str, from: i64, to: i64) -> Result<Vec<Row>> {
    if cache.covered(symbol, from, to)? {
        return cache.get(symbol, from, to);
    }
    let rows = api::fetch_quotes(cfg, Some(symbol), from, to)?;
    let now = chrono::Utc::now().timestamp();
    let covered_to = to.min(now - 3600);
    if covered_to > from {
        cache.put(symbol, from, covered_to, &rows)?;
    }
    Ok(rows)
}
