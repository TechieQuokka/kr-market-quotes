import type { Env, Quote, Sym } from "./types";

const HEADERS = { "User-Agent": "Mozilla/5.0 (kr-market-quotes collector)" };

const FX_URL =
  "https://m.stock.naver.com/front-api/marketIndex/productDetail?category=exchange&reutersCode=FX_USDKRW";

// "6,856.83" → 6856.83
function parseValue(s: string): number {
  const n = Number(s.replace(/,/g, ""));
  if (!Number.isFinite(n)) throw new Error(`숫자 파싱 실패: ${s}`);
  return n;
}

// "2026-07-14T18:59:00+09:00" → epoch초
function toEpoch(localTradedAt: string): number {
  const ms = Date.parse(localTradedAt);
  if (Number.isNaN(ms)) throw new Error(`시각 파싱 실패: ${localTradedAt}`);
  return Math.floor(ms / 1000);
}

async function fetchIndex(code: "KOSPI" | "KOSDAQ"): Promise<Quote> {
  const res = await fetch(`https://m.stock.naver.com/api/index/${code}/basic`, {
    headers: HEADERS,
  });
  if (!res.ok) throw new Error(`${code} HTTP ${res.status}`);
  const j = (await res.json()) as { closePrice: string; localTradedAt: string };
  return { symbol: code, ts: toEpoch(j.localTradedAt), value: parseValue(j.closePrice) };
}

async function fetchUsdKrw(): Promise<Quote> {
  const res = await fetch(FX_URL, { headers: HEADERS });
  if (!res.ok) throw new Error(`USDKRW HTTP ${res.status}`);
  const j = (await res.json()) as {
    result: { closePrice: string; localTradedAt: string };
  };
  return {
    symbol: "USDKRW",
    ts: toEpoch(j.result.localTradedAt),
    value: parseValue(j.result.closePrice),
  };
}

// 이번 시각에 수집할 대상. cron은 UTC로 돌므로 여기서 KST로 판단한다.
export function targets(now: Date): Sym[] {
  const kst = new Date(now.getTime() + 9 * 3600 * 1000);
  const day = kst.getUTCDay(); // 0=일 … 6=토
  const hour = kst.getUTCHours();

  // KRX 정규장: 평일 09:00~15:30. 16시 정각까지 돌려 확정 종가를 담는다.
  const krxOpen = day >= 1 && day <= 5 && hour >= 9 && hour <= 16;

  // 원/달러: 월 06:00 ~ 토 06:00 KST 연속 개장 (2026-07 외환시장 24시간 개방)
  const fxOpen =
    (day >= 1 && day <= 5 && !(day === 1 && hour < 6)) || (day === 6 && hour < 6);

  const t: Sym[] = [];
  if (krxOpen) t.push("KOSPI", "KOSDAQ");
  if (fxOpen) t.push("USDKRW");
  return t;
}

// 심볼별 최신 저장값. 값이 그대로면 새로 쓰지 않기 위해 필요하다.
//
// (symbol, ts) PK + INSERT OR IGNORE 만으로는 중복이 걸러지지 않는다.
// 그 방식은 "값이 그대로면 localTradedAt도 그대로"를 전제하는데, 개장 09:00 정각에는
// 네이버가 localTradedAt만 새로 찍고 closePrice는 아직 전일 종가를 준다.
// 그래서 ts는 새 값 · value는 전일 종가인 행이 매일 하나씩 들어왔다.
async function lastValues(env: Env, syms: Sym[]): Promise<Map<string, number>> {
  const ph = syms.map(() => "?").join(",");
  const { results } = await env.DB.prepare(
    `SELECT q.symbol, q.value FROM quotes q
     JOIN (SELECT symbol, MAX(ts) AS ts FROM quotes WHERE symbol IN (${ph}) GROUP BY symbol) m
       ON q.symbol = m.symbol AND q.ts = m.ts`,
  )
    .bind(...syms)
    .all<{ symbol: string; value: number }>();
  return new Map(results.map((r) => [r.symbol, r.value]));
}

export type Outcome = "saved" | "unchanged" | "failed";

export interface CollectResult {
  saved: number;
  unchanged: number;
  failed: string[];
  outcomes: Record<string, Outcome>;
}

export async function collect(env: Env, now: Date): Promise<CollectResult> {
  const syms = targets(now);
  const empty: CollectResult = { saved: 0, unchanged: 0, failed: [], outcomes: {} };
  if (syms.length === 0) return empty;

  const results = await Promise.allSettled(
    syms.map((s) => (s === "USDKRW" ? fetchUsdKrw() : fetchIndex(s))),
  );

  const fetched: Quote[] = [];
  const failed: string[] = [];
  const outcomes: Record<string, Outcome> = {};
  for (const [i, r] of results.entries()) {
    if (r.status === "fulfilled") fetched.push(r.value);
    else {
      failed.push(syms[i]);
      outcomes[syms[i]] = "failed";
      console.error(`${syms[i]} 수집 실패:`, r.reason);
    }
  }

  // 값이 직전 저장분과 같으면 저장하지 않는다 (수집이 돌았다는 사실은 collect_log에 남는다).
  const last = await lastValues(env, syms);
  const fresh: Quote[] = [];
  for (const q of fetched) {
    if (last.get(q.symbol) === q.value) {
      outcomes[q.symbol] = "unchanged";
    } else {
      outcomes[q.symbol] = "saved";
      fresh.push(q);
    }
  }

  if (fresh.length > 0) {
    const stmt = env.DB.prepare(
      "INSERT OR IGNORE INTO quotes (symbol, ts, value) VALUES (?, ?, ?)",
    );
    await env.DB.batch(fresh.map((q) => stmt.bind(q.symbol, q.ts, q.value)));
  }

  await writeLog(env, now, outcomes);
  return { saved: fresh.length, unchanged: fetched.length - fresh.length, failed, outcomes };
}

// 매 수집 실행의 심볼별 결과를 남긴다. 이게 없으면 "행이 없다"가
// 수집 실패인지 값 미변동인지 사후에 구분할 방법이 없다.
async function writeLog(env: Env, now: Date, outcomes: Record<string, Outcome>): Promise<void> {
  const runTs = Math.floor(now.getTime() / 1000);
  const entries = Object.entries(outcomes);
  if (entries.length === 0) return;
  const stmt = env.DB.prepare(
    "INSERT OR REPLACE INTO collect_log (run_ts, symbol, outcome) VALUES (?, ?, ?)",
  );
  try {
    await env.DB.batch([
      ...entries.map(([sym, o]) => stmt.bind(runTs, sym, o)),
      // 90일 초과분 정리 — 로그가 무한히 자라지 않도록.
      env.DB.prepare("DELETE FROM collect_log WHERE run_ts < ?").bind(runTs - 90 * 86400),
    ]);
  } catch (e) {
    console.error("collect_log 기록 실패:", e);
  }
}
