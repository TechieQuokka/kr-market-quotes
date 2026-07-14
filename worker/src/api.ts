import { collect } from "./collect";
import type { Env } from "./types";

// 미인증 요청은 401이 아닌 404 — 외부에서 보면 존재하지 않는 주소처럼 보이게.
const NOT_FOUND = () => new Response("Not Found", { status: 404 });

function authorized(req: Request, env: Env): boolean {
  const auth = req.headers.get("Authorization") ?? "";
  return env.API_KEY.length > 0 && auth === `Bearer ${env.API_KEY}`;
}

// GET /quotes?symbol=KOSPI&from=<epoch초>&to=<epoch초>  (셋 다 생략 가능)
async function handleQuotes(url: URL, env: Env): Promise<Response> {
  const symbol = url.searchParams.get("symbol");
  const from = Number(url.searchParams.get("from") ?? 0);
  const to = Number(url.searchParams.get("to") ?? Number.MAX_SAFE_INTEGER);
  if (!Number.isFinite(from) || !Number.isFinite(to)) {
    return new Response("bad range", { status: 400 });
  }

  const { results } = await env.DB.prepare(
    `SELECT symbol, ts, value FROM quotes
     WHERE (?1 IS NULL OR symbol = ?1) AND ts >= ?2 AND ts <= ?3
     ORDER BY symbol, ts`,
  )
    .bind(symbol, from, to)
    .all();

  return Response.json(results);
}

export async function handleRequest(req: Request, env: Env): Promise<Response> {
  if (!authorized(req, env)) return NOT_FOUND();

  const url = new URL(req.url);

  if (req.method === "GET" && url.pathname === "/quotes") {
    return handleQuotes(url, env);
  }

  // 수동 수집 트리거 (네이버 장애 복구 후 즉시 재수집 등 운영용)
  if (req.method === "POST" && url.pathname === "/collect") {
    const r = await collect(env, new Date());
    return Response.json(r);
  }

  return NOT_FOUND();
}
