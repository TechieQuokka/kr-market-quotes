CREATE TABLE IF NOT EXISTS quotes (
  symbol TEXT NOT NULL,    -- 'KOSPI' | 'KOSDAQ' | 'USDKRW'
  ts     INTEGER NOT NULL, -- 네이버 응답의 localTradedAt (unix epoch, 초)
  value  REAL NOT NULL,    -- 지수 포인트 or 원/달러 환율
  PRIMARY KEY (symbol, ts)
) WITHOUT ROWID;

-- 매 수집 실행의 심볼별 결과. quotes에 행이 없을 때
-- '수집 실패'인지 '값이 안 변해서 안 씀'인지 구분하기 위한 것.
CREATE TABLE IF NOT EXISTS collect_log (
  run_ts  INTEGER NOT NULL, -- 수집 실행 시각 (unix epoch, 초)
  symbol  TEXT    NOT NULL,
  outcome TEXT    NOT NULL, -- 'saved' | 'unchanged' | 'failed'
  PRIMARY KEY (run_ts, symbol)
) WITHOUT ROWID;
