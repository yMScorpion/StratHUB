# exec-rs

Single Rust binary that runs a Strategy Spec in one of three modes — `backtest`, `paper`, or `live`. The same interpreter, decimals, and risk-guard semantics are used in all three; only the data source and order sink differ.

## Run

```
cargo run -p exec-rs -- --mode backtest --spec ../../packages/strategy-spec/fixtures/wyckoff_spring_btc_15m.json
```

Phase 4 backtests also require a snapshot-versioned market data file. The JSON fixture mirrors the
DuckDB+Parquet metadata contract used by Supabase `market_data_snapshots` rows: `data_snapshot_id`,
fee/slippage/funding models, contract specs, lot/min notional, leverage/margin, liquidation, and
delisting inputs are all part of the reproducibility boundary.

```
cargo run -p exec-rs -- --mode backtest \
  --spec packages/strategy-spec/fixtures/wyckoff_spring_btc_15m.json \
  --market-data apps/exec-rs/fixtures/binance_btcusdt_15m_snapshot_2024_01_sample.json \
  --data-snapshot-id binance-btcusdt-15m-2024-01-real-sample-v1
```

The command prints a deterministic JSON report containing `kpi_jsonb.meta`, trades, equity curve,
and heatmap. Logs are written to stderr so stdout remains machine-readable.

## Why one binary

Drift between research and live is the #1 source of "it worked in the backtest" failures. By forcing all three modes through the same compiled code path consuming the same immutable `spec_hash`, we eliminate that drift by construction.
