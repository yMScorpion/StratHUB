# exec-rs

Single Rust binary that runs a Strategy Spec in one of three modes — `backtest`, `paper`, or `live`. The same interpreter, decimals, and risk-guard semantics are used in all three; only the data source and order sink differ.

## Run

```
cargo run -p exec-rs -- --mode backtest --spec ../../packages/strategy-spec/fixtures/wyckoff_spring_btc_15m.json
```

## Why one binary

Drift between research and live is the #1 source of "it worked in the backtest" failures. By forcing all three modes through the same compiled code path consuming the same immutable `spec_hash`, we eliminate that drift by construction.
