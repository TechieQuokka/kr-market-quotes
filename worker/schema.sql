CREATE TABLE IF NOT EXISTS quotes (
  symbol TEXT NOT NULL,    -- 'KOSPI' | 'KOSDAQ' | 'USDKRW'
  ts     INTEGER NOT NULL, -- 네이버 응답의 localTradedAt (unix epoch, 초)
  value  REAL NOT NULL,    -- 지수 포인트 or 원/달러 환율
  PRIMARY KEY (symbol, ts)
) WITHOUT ROWID;
