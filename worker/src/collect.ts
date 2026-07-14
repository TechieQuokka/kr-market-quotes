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

export async function collect(env: Env, now: Date): Promise<{ saved: number; failed: string[] }> {
  const syms = targets(now);
  if (syms.length === 0) return { saved: 0, failed: [] };

  const results = await Promise.allSettled(
    syms.map((s) => (s === "USDKRW" ? fetchUsdKrw() : fetchIndex(s))),
  );

  const quotes: Quote[] = [];
  const failed: string[] = [];
  for (const [i, r] of results.entries()) {
    if (r.status === "fulfilled") quotes.push(r.value);
    else {
      failed.push(syms[i]);
      console.error(`${syms[i]} 수집 실패:`, r.reason);
    }
  }

  if (quotes.length > 0) {
    const stmt = env.DB.prepare(
      "INSERT OR IGNORE INTO quotes (symbol, ts, value) VALUES (?, ?, ?)",
    );
    await env.DB.batch(quotes.map((q) => stmt.bind(q.symbol, q.ts, q.value)));
  }
  return { saved: quotes.length, failed };
}
