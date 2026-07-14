export interface Env {
  DB: D1Database;
  API_KEY: string;
}

export type Sym = "KOSPI" | "KOSDAQ" | "USDKRW";

export interface Quote {
  symbol: Sym;
  ts: number; // 네이버 localTradedAt (unix epoch, 초)
  value: number;
}

export const SYMBOLS: Sym[] = ["KOSPI", "KOSDAQ", "USDKRW"];
