use crate::compute::{Enriched, fmt_kst};
use anyhow::Result;
use serde::Serialize;
use std::io::Write;

/// 첫 컬럼 좌측 정렬, 나머지 우측 정렬의 단순 테이블
pub fn print_table(headers: &[&str], rows: &[Vec<String>]) {
    let cols = headers.len();
    let mut widths: Vec<usize> = headers.iter().map(|h| h.len()).collect();
    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            widths[i] = widths[i].max(cell.len());
        }
    }
    let line = |cells: Vec<&str>| {
        let mut s = String::new();
        for i in 0..cols {
            if i == 0 {
                s.push_str(&format!("{:<w$}", cells[i], w = widths[i]));
            } else {
                s.push_str(&format!("  {:>w$}", cells[i], w = widths[i]));
            }
        }
        s
    };
    println!("{}", line(headers.to_vec()));
    for row in rows {
        println!("{}", line(row.iter().map(String::as_str).collect()));
    }
}

pub fn fmt_delta(v: Option<f64>) -> String {
    v.map(|d| format!("{d:+.2}")).unwrap_or_else(|| "-".into())
}

pub fn fmt_pct(v: Option<f64>) -> String {
    v.map(|d| format!("{d:+.2}%")).unwrap_or_else(|| "-".into())
}

#[derive(Serialize)]
struct ExportRow<'a> {
    symbol: &'a str,
    ts: i64,
    time_kst: String,
    value: f64,
    delta: Option<f64>,
    delta_pct: Option<f64>,
}

fn export_row(e: &Enriched) -> ExportRow<'_> {
    ExportRow {
        symbol: &e.row.symbol,
        ts: e.row.ts,
        time_kst: fmt_kst(e.row.ts),
        value: e.row.value,
        delta: e.delta,
        delta_pct: e.delta_pct.map(|p| (p * 10000.0).round() / 10000.0),
    }
}

pub fn write_jsonl<W: Write>(w: &mut W, rows: &[Enriched]) -> Result<()> {
    for e in rows {
        serde_json::to_writer(&mut *w, &export_row(e))?;
        writeln!(w)?;
    }
    Ok(())
}

pub fn write_csv<W: Write>(w: &mut W, rows: &[Enriched]) -> Result<()> {
    writeln!(w, "symbol,ts,time_kst,value,delta,delta_pct")?;
    for e in rows {
        let r = export_row(e);
        writeln!(
            w,
            "{},{},{},{},{},{}",
            r.symbol,
            r.ts,
            r.time_kst,
            r.value,
            r.delta.map(|v| v.to_string()).unwrap_or_default(),
            r.delta_pct.map(|v| v.to_string()).unwrap_or_default(),
        )?;
    }
    Ok(())
}

pub fn write_json<W: Write>(w: &mut W, rows: &[Enriched]) -> Result<()> {
    let out: Vec<ExportRow> = rows.iter().map(export_row).collect();
    serde_json::to_writer_pretty(&mut *w, &out)?;
    writeln!(w)?;
    Ok(())
}
