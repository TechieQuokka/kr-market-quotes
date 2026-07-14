# kr-market-quotes

Collects KOSPI, KOSDAQ, and USD/KRW quotes every hour into a tiny database (indefinitely, on the Cloudflare free tier) and provides a Rust CLI for querying and analysis.

## Architecture

- **worker/** — Cloudflare Worker. An hourly cron fetches quotes from Naver Finance JSON endpoints and stores them in D1. It also exposes a single authenticated query endpoint. Nothing else.
- **cli/** — `kmq` (Rust). Every feature lives here: queries, trend analysis, exports, health checks. Derived values (deltas, trends, aggregates) are always computed at read time — the database stores raw quotes only, by design principle.

## Collection schedule (KST)

| Symbol | Window |
|---|---|
| KOSPI, KOSDAQ | Weekdays 09:00–16:00 (KRX regular session 09:00–15:30) |
| USDKRW | Mon 06:00 – Sat 06:00, continuous (FX market runs 24h on weekdays since July 2026) |

The database is a single table: `quotes(symbol, ts, value)`. `ts` is Naver's `localTradedAt`, so when the source value hasn't changed, `INSERT OR IGNORE` deduplicates automatically.

## kmq usage

```sh
kmq latest                                        # latest value of all three symbols
kmq fetch  --symbol KOSPI --from 2026-07-01       # range query (with Δ/Δ% vs previous sample)
kmq trend  --symbol KOSPI --month 2026-07         # up / down / flat verdict
kmq export --format csv --out backup.csv          # export everything (doubles as a backup)
kmq status                                        # collection health check (exit 1 on anomaly)
kmq cache info | clear                            # local cache management
```

Run `kmq --help` or `kmq <command> --help` for details.

Config lives at `~/.config/kmq/config.toml` (`api_url`, `api_key`). Historical ranges are cached in `~/.cache/kmq/cache.db` and never re-fetched (past quotes are immutable).

## Operating habits

- Run `kmq status` occasionally — it is the only safety net that detects silent collection failures (e.g. Naver changing its API).
- Run `kmq export --out backup.jsonl` occasionally for a local backup.
- To re-collect after an outage: `curl -X POST -H "Authorization: Bearer <key>" <api_url>/collect`

## Deploy / operations (worker/)

```sh
npm run deploy        # deploy
npm run tail          # live logs
npx wrangler d1 execute kr-market-quotes --remote --command "SELECT COUNT(*) FROM quotes"
```
