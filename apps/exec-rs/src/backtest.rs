use std::collections::BTreeMap;
use std::path::Path;
use std::str::FromStr;

use anyhow::{Context, Result};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use strategy_spec::hash_spec;

#[derive(Debug, Clone, Deserialize)]
pub struct MarketSnapshot {
    pub data_snapshot_id: String,
    #[serde(default)]
    pub venue: String,
    #[serde(default)]
    pub symbol: String,
    #[serde(default)]
    pub timeframe: String,
    #[serde(default)]
    pub fees: FeeModel,
    #[serde(default)]
    pub slippage: SlippageModel,
    #[serde(default)]
    pub funding: FundingModel,
    #[serde(default)]
    pub contract: ContractSpec,
    #[serde(default)]
    pub candles: Vec<Candle>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Candle {
    pub ts: String,
    #[serde(with = "rust_decimal::serde::str")]
    pub open: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    pub high: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    pub low: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    pub close: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    pub volume: Decimal,
    #[serde(default, with = "rust_decimal::serde::str_option")]
    pub spread_bps: Option<Decimal>,
    #[serde(default)]
    pub delisted: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FeeModel {
    #[serde(default = "default_maker_bps", with = "rust_decimal::serde::str")]
    pub maker_bps: Decimal,
    #[serde(default = "default_taker_bps", with = "rust_decimal::serde::str")]
    pub taker_bps: Decimal,
}

impl Default for FeeModel {
    fn default() -> Self {
        Self {
            maker_bps: default_maker_bps(),
            taker_bps: default_taker_bps(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct SlippageModel {
    #[serde(default = "default_slippage_bps", with = "rust_decimal::serde::str")]
    pub bps: Decimal,
    #[serde(default = "default_intrabar")]
    pub intrabar: IntrabarAssumption,
}

impl Default for SlippageModel {
    fn default() -> Self {
        Self {
            bps: default_slippage_bps(),
            intrabar: default_intrabar(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IntrabarAssumption {
    Conservative,
    Optimistic,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct FundingModel {
    #[serde(default, with = "rust_decimal::serde::str")]
    pub bps_per_day: Decimal,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ContractSpec {
    #[serde(default = "default_tick_size", with = "rust_decimal::serde::str")]
    pub tick_size: Decimal,
    #[serde(default = "default_lot_size", with = "rust_decimal::serde::str")]
    pub lot_size: Decimal,
    #[serde(default = "default_min_notional", with = "rust_decimal::serde::str")]
    pub min_notional: Decimal,
    #[serde(default = "default_max_leverage", with = "rust_decimal::serde::str")]
    pub max_leverage: Decimal,
    #[serde(default = "default_maintenance_margin_bps", with = "rust_decimal::serde::str")]
    pub maintenance_margin_bps: Decimal,
}

impl Default for ContractSpec {
    fn default() -> Self {
        Self {
            tick_size: default_tick_size(),
            lot_size: default_lot_size(),
            min_notional: default_min_notional(),
            max_leverage: default_max_leverage(),
            maintenance_margin_bps: default_maintenance_margin_bps(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct BacktestConfig {
    pub initial_equity: Decimal,
    pub data_snapshot_id: String,
    pub seed: u64,
    pub image_digest: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct BacktestReport {
    pub spec_hash: String,
    pub data_snapshot_id: String,
    pub kpi_jsonb: KpiBundle,
    pub equity_curve: Vec<EquityPoint>,
    pub heatmap: BTreeMap<String, HeatmapCell>,
    pub trades: Vec<Trade>,
}

#[derive(Debug, Clone, Serialize)]
pub struct KpiBundle {
    pub metrics: BTreeMap<String, String>,
    pub meta: BacktestMeta,
}

#[derive(Debug, Clone, Serialize)]
pub struct BacktestMeta {
    pub data_snapshot_id: String,
    pub fee_model: String,
    pub slippage_model: String,
    pub intrabar_assumption: IntrabarAssumption,
    pub funding_model: String,
    pub contract_spec: String,
    pub lib_versions: BTreeMap<String, String>,
    pub image_digest: String,
    pub seeds: Vec<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EquityPoint {
    pub ts: String,
    pub equity: String,
    pub drawdown_pct: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct HeatmapCell {
    pub trades: usize,
    pub pnl: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Trade {
    pub entry_ts: String,
    pub exit_ts: String,
    pub symbol: String,
    pub side: String,
    pub qty: String,
    pub entry_price: String,
    pub exit_price: String,
    pub stop_price: String,
    pub take_profit_price: String,
    pub gross_pnl: String,
    pub fees: String,
    pub net_pnl: String,
    pub rr: String,
    pub exit_reason: String,
}

#[derive(Debug, Clone)]
struct Position {
    entry_ts: String,
    qty: Decimal,
    entry_price: Decimal,
    stop_price: Decimal,
    take_profit_price: Decimal,
    risk_amount: Decimal,
}

pub fn load_market_snapshot(path: &Path) -> Result<MarketSnapshot> {
    let raw = std::fs::read(path).with_context(|| format!("read market data {}", path.display()))?;
    let snapshot: MarketSnapshot = serde_json::from_slice(&raw).context("parse market snapshot JSON")?;
    anyhow::ensure!(
        snapshot.candles.len() >= 30,
        "market snapshot needs at least 30 candles for deterministic indicators"
    );
    Ok(snapshot)
}

pub fn run_backtest(spec: &Value, snapshot: &MarketSnapshot, config: BacktestConfig) -> Result<BacktestReport> {
    let spec_hash = hash_spec(spec);
    anyhow::ensure!(
        config.data_snapshot_id == snapshot.data_snapshot_id,
        "requested data_snapshot_id {} does not match snapshot {}",
        config.data_snapshot_id,
        snapshot.data_snapshot_id
    );

    let symbol = spec
        .get("symbols")
        .and_then(Value::as_array)
        .and_then(|xs| xs.first())
        .and_then(Value::as_str)
        .unwrap_or(&snapshot.symbol)
        .to_string();
    let risk_pct = parse_decimal_path(spec, &["risk", "per_trade_pct"]).unwrap_or(dec!(0.5)) / dec!(100);
    let min_rr = parse_decimal_path(spec, &["risk", "min_rr"]).unwrap_or(dec!(3));
    let max_spread_bps = max_spread_bps(spec).unwrap_or(dec!(999999));
    let stop_mult = exit_param(spec, "stop_atr", "mult").unwrap_or(dec!(1.5));
    let tp_rr = exit_param(spec, "tp_rr", "rr").unwrap_or(min_rr);

    let atr = atr14(&snapshot.candles);
    let volume_ma = volume_ma(&snapshot.candles, 20);
    let mut equity = config.initial_equity;
    let mut peak = equity;
    let mut position: Option<Position> = None;
    let mut trades = Vec::new();
    let mut equity_curve = Vec::new();

    for i in 1..snapshot.candles.len() {
        let c = &snapshot.candles[i];
        if c.delisted {
            if let Some(pos) = position.take() {
                close_position(
                    &mut equity,
                    &mut trades,
                    &symbol,
                    pos,
                    c,
                    execution_price(c.close, snapshot.slippage.bps, false),
                    "delisting",
                    snapshot,
                );
            }
            push_equity(&mut equity_curve, c, equity, &mut peak);
            continue;
        }

        if let Some(pos) = position.take() {
            let hit_stop = c.low <= pos.stop_price;
            let hit_tp = c.high >= pos.take_profit_price;
            let close = if hit_stop && hit_tp {
                match snapshot.slippage.intrabar {
                    IntrabarAssumption::Conservative => Some((pos.stop_price, "stop_loss")),
                    IntrabarAssumption::Optimistic => Some((pos.take_profit_price, "take_profit")),
                }
            } else if hit_stop {
                Some((pos.stop_price, "stop_loss"))
            } else if hit_tp {
                Some((pos.take_profit_price, "take_profit"))
            } else {
                None
            };

            if let Some((price, reason)) = close {
                close_position(
                    &mut equity,
                    &mut trades,
                    &symbol,
                    pos,
                    c,
                    execution_price(price, snapshot.slippage.bps, false),
                    reason,
                    snapshot,
                );
            } else {
                position = Some(pos);
            }
        }

        if position.is_none()
            && i >= 20
            && c.spread_bps.unwrap_or_default() <= max_spread_bps
            && wyckoff_spring(&snapshot.candles, &atr, i)
            && vsa_no_supply(&snapshot.candles, &volume_ma, i)
        {
            let entry_price = execution_price(c.close, snapshot.slippage.bps, true);
            let stop_distance = atr[i].unwrap_or(dec!(0)) * stop_mult;
            if stop_distance > dec!(0) {
                let stop_price = round_down(entry_price - stop_distance, snapshot.contract.tick_size);
                let take_profit_price = round_down(entry_price + stop_distance * tp_rr, snapshot.contract.tick_size);
                let risk_amount = equity * risk_pct;
                let raw_qty = risk_amount / (entry_price - stop_price);
                let qty = round_down(raw_qty, snapshot.contract.lot_size);
                if qty * entry_price >= snapshot.contract.min_notional && qty > dec!(0) {
                    position = Some(Position {
                        entry_ts: c.ts.clone(),
                        qty,
                        entry_price,
                        stop_price,
                        take_profit_price,
                        risk_amount,
                    });
                }
            }
        }

        push_equity(&mut equity_curve, c, equity, &mut peak);
    }

    if let Some(pos) = position.take() {
        let last = snapshot.candles.last().expect("snapshot has candles");
        close_position(
            &mut equity,
            &mut trades,
            &symbol,
            pos,
            last,
            execution_price(last.close, snapshot.slippage.bps, false),
            "end_of_data",
            snapshot,
        );
        equity_curve.pop();
        push_equity(&mut equity_curve, last, equity, &mut peak);
    }

    let heatmap = build_heatmap(&trades);
    let kpi_jsonb = build_kpis(
        config,
        snapshot,
        &trades,
        &equity_curve,
        spec_hash.clone(),
        symbol,
    );

    Ok(BacktestReport {
        spec_hash,
        data_snapshot_id: snapshot.data_snapshot_id.clone(),
        kpi_jsonb,
        equity_curve,
        heatmap,
        trades,
    })
}

fn close_position(
    equity: &mut Decimal,
    trades: &mut Vec<Trade>,
    symbol: &str,
    pos: Position,
    candle: &Candle,
    exit_price: Decimal,
    exit_reason: &str,
    snapshot: &MarketSnapshot,
) {
    let notional_in = pos.qty * pos.entry_price;
    let notional_out = pos.qty * exit_price;
    let gross_pnl = (exit_price - pos.entry_price) * pos.qty;
    let fees = (notional_in + notional_out) * snapshot.fees.taker_bps / dec!(10000);
    let net_pnl = gross_pnl - fees;
    *equity += net_pnl;
    let rr = if pos.risk_amount > dec!(0) {
        net_pnl / pos.risk_amount
    } else {
        dec!(0)
    };

    trades.push(Trade {
        entry_ts: pos.entry_ts,
        exit_ts: candle.ts.clone(),
        symbol: symbol.to_string(),
        side: "long".to_string(),
        qty: fmt(pos.qty),
        entry_price: fmt(pos.entry_price),
        exit_price: fmt(exit_price),
        stop_price: fmt(pos.stop_price),
        take_profit_price: fmt(pos.take_profit_price),
        gross_pnl: fmt(gross_pnl),
        fees: fmt(fees),
        net_pnl: fmt(net_pnl),
        rr: fmt(rr),
        exit_reason: exit_reason.to_string(),
    });
}

fn build_kpis(
    config: BacktestConfig,
    snapshot: &MarketSnapshot,
    trades: &[Trade],
    curve: &[EquityPoint],
    spec_hash: String,
    symbol: String,
) -> KpiBundle {
    let pnls: Vec<Decimal> = trades
        .iter()
        .map(|t| Decimal::from_str(&t.net_pnl).unwrap_or(dec!(0)))
        .collect();
    let wins: Vec<Decimal> = pnls.iter().copied().filter(|x| *x > dec!(0)).collect();
    let losses: Vec<Decimal> = pnls.iter().copied().filter(|x| *x < dec!(0)).collect();
    let total_pnl: Decimal = pnls.iter().sum();
    let gross_profit: Decimal = wins.iter().sum();
    let gross_loss: Decimal = losses.iter().map(|x| x.abs()).sum();
    let win_rate = if trades.is_empty() {
        dec!(0)
    } else {
        Decimal::from(wins.len() as u64) / Decimal::from(trades.len() as u64)
    };
    let expectancy = if trades.is_empty() {
        dec!(0)
    } else {
        total_pnl / Decimal::from(trades.len() as u64)
    };
    let avg_rr = average(
        &trades
            .iter()
            .map(|t| Decimal::from_str(&t.rr).unwrap_or(dec!(0)))
            .collect::<Vec<_>>(),
    );
    let returns = equity_returns(curve);
    let downside: Vec<Decimal> = returns.iter().copied().filter(|x| *x < dec!(0)).collect();
    let sharpe = ratio(average(&returns), stddev(&returns)) * dec!(100);
    let sortino = ratio(average(&returns), stddev(&downside)) * dec!(100);
    let max_dd = curve
        .iter()
        .filter_map(|p| Decimal::from_str(&p.drawdown_pct).ok())
        .max()
        .unwrap_or(dec!(0));
    let calmar = ratio(total_pnl / config.initial_equity, max_dd / dec!(100));
    let profit_factor = ratio(gross_profit, gross_loss);
    let cagr = total_pnl / config.initial_equity;
    let time_in_market = if curve.is_empty() {
        dec!(0)
    } else {
        Decimal::from(trades.len() as u64) / Decimal::from(curve.len() as u64)
    };

    let mut metrics = BTreeMap::new();
    metrics.insert("sharpe".to_string(), fmt(sharpe));
    metrics.insert("sortino".to_string(), fmt(sortino));
    metrics.insert("calmar".to_string(), fmt(calmar));
    metrics.insert("max_dd_pct".to_string(), fmt(max_dd));
    metrics.insert("profit_factor".to_string(), fmt(profit_factor));
    metrics.insert("win_rate".to_string(), fmt(win_rate));
    metrics.insert("avg_rr".to_string(), fmt(avg_rr));
    metrics.insert("expectancy".to_string(), fmt(expectancy));
    metrics.insert("cagr".to_string(), fmt(cagr));
    metrics.insert("time_in_market".to_string(), fmt(time_in_market));
    metrics.insert("net_pnl".to_string(), fmt(total_pnl));
    metrics.insert("trade_count".to_string(), trades.len().to_string());

    let mut lib_versions = BTreeMap::new();
    lib_versions.insert("exec-rs".to_string(), env!("CARGO_PKG_VERSION").to_string());
    lib_versions.insert("strategy-spec".to_string(), "workspace".to_string());
    lib_versions.insert("rust_decimal".to_string(), "1.36".to_string());
    lib_versions.insert("spec_hash".to_string(), spec_hash);
    lib_versions.insert("symbol".to_string(), symbol);
    lib_versions.insert("venue".to_string(), snapshot.venue.clone());
    lib_versions.insert("timeframe".to_string(), snapshot.timeframe.clone());

    KpiBundle {
        metrics,
        meta: BacktestMeta {
            data_snapshot_id: snapshot.data_snapshot_id.clone(),
            fee_model: format!("maker_bps={},taker_bps={}", fmt(snapshot.fees.maker_bps), fmt(snapshot.fees.taker_bps)),
            slippage_model: format!("bps={}", fmt(snapshot.slippage.bps)),
            intrabar_assumption: snapshot.slippage.intrabar.clone(),
            funding_model: format!("bps_per_day={}", fmt(snapshot.funding.bps_per_day)),
            contract_spec: format!(
                "tick_size={},lot_size={},min_notional={},max_leverage={},maintenance_margin_bps={}",
                fmt(snapshot.contract.tick_size),
                fmt(snapshot.contract.lot_size),
                fmt(snapshot.contract.min_notional),
                fmt(snapshot.contract.max_leverage),
                fmt(snapshot.contract.maintenance_margin_bps),
            ),
            lib_versions,
            image_digest: config.image_digest,
            seeds: vec![config.seed],
        },
    }
}

fn build_heatmap(trades: &[Trade]) -> BTreeMap<String, HeatmapCell> {
    let mut out: BTreeMap<String, HeatmapCell> = BTreeMap::new();
    for trade in trades {
        let key = trade.exit_ts.get(0..7).unwrap_or("unknown").to_string();
        let pnl = Decimal::from_str(&trade.net_pnl).unwrap_or(dec!(0));
        let cell = out.entry(key).or_insert(HeatmapCell {
            trades: 0,
            pnl: fmt(dec!(0)),
        });
        cell.trades += 1;
        let next = Decimal::from_str(&cell.pnl).unwrap_or(dec!(0)) + pnl;
        cell.pnl = fmt(next);
    }
    out
}

fn push_equity(curve: &mut Vec<EquityPoint>, candle: &Candle, equity: Decimal, peak: &mut Decimal) {
    if equity > *peak {
        *peak = equity;
    }
    let dd = if *peak > dec!(0) {
        ((*peak - equity) / *peak) * dec!(100)
    } else {
        dec!(0)
    };
    curve.push(EquityPoint {
        ts: candle.ts.clone(),
        equity: fmt(equity),
        drawdown_pct: fmt(dd),
    });
}

fn atr14(candles: &[Candle]) -> Vec<Option<Decimal>> {
    let mut true_ranges = vec![dec!(0); candles.len()];
    for i in 1..candles.len() {
        let c = &candles[i];
        let prev_close = candles[i - 1].close;
        true_ranges[i] = (c.high - c.low)
            .max((c.high - prev_close).abs())
            .max((c.low - prev_close).abs());
    }
    let mut out = vec![None; candles.len()];
    for i in 14..candles.len() {
        let sum: Decimal = true_ranges[i - 13..=i].iter().sum();
        out[i] = Some(sum / dec!(14));
    }
    out
}

fn volume_ma(candles: &[Candle], length: usize) -> Vec<Option<Decimal>> {
    let mut out = vec![None; candles.len()];
    for i in length..candles.len() {
        let sum: Decimal = candles[i + 1 - length..=i].iter().map(|c| c.volume).sum();
        out[i] = Some(sum / Decimal::from(length as u64));
    }
    out
}

fn wyckoff_spring(candles: &[Candle], atr: &[Option<Decimal>], i: usize) -> bool {
    if i < 20 {
        return false;
    }
    let c = &candles[i];
    let prior_low = candles[i - 20..i]
        .iter()
        .map(|x| x.low)
        .min()
        .unwrap_or(c.low);
    let Some(atr) = atr[i] else {
        return false;
    };
    c.low < prior_low && c.close > prior_low && (prior_low - c.low) <= atr * dec!(0.5)
}

fn vsa_no_supply(candles: &[Candle], volume_ma: &[Option<Decimal>], i: usize) -> bool {
    let c = &candles[i];
    let Some(ma) = volume_ma[i] else {
        return false;
    };
    c.close < c.open && c.volume <= ma * dec!(0.7)
}

fn equity_returns(curve: &[EquityPoint]) -> Vec<Decimal> {
    let mut out = Vec::new();
    for pair in curve.windows(2) {
        let prev = Decimal::from_str(&pair[0].equity).unwrap_or(dec!(0));
        let next = Decimal::from_str(&pair[1].equity).unwrap_or(dec!(0));
        if prev > dec!(0) {
            out.push((next - prev) / prev);
        }
    }
    out
}

fn average(xs: &[Decimal]) -> Decimal {
    if xs.is_empty() {
        dec!(0)
    } else {
        xs.iter().copied().sum::<Decimal>() / Decimal::from(xs.len() as u64)
    }
}

fn stddev(xs: &[Decimal]) -> Decimal {
    if xs.len() < 2 {
        return dec!(0);
    }
    let avg = average(xs);
    let variance = xs
        .iter()
        .map(|x| {
            let d = *x - avg;
            d * d
        })
        .sum::<Decimal>()
        / Decimal::from((xs.len() - 1) as u64);
    decimal_sqrt(variance)
}

fn decimal_sqrt(n: Decimal) -> Decimal {
    if n <= dec!(0) {
        return dec!(0);
    }
    let mut x = Decimal::from_f64_retain(n.to_f64().unwrap_or(0.0).sqrt()).unwrap_or(dec!(1));
    for _ in 0..16 {
        x = (x + n / x) / dec!(2);
    }
    x
}

fn ratio(n: Decimal, d: Decimal) -> Decimal {
    if d == dec!(0) {
        dec!(0)
    } else {
        n / d
    }
}

fn execution_price(price: Decimal, slippage_bps: Decimal, buy: bool) -> Decimal {
    let adj = price * slippage_bps / dec!(10000);
    if buy {
        price + adj
    } else {
        price - adj
    }
}

fn round_down(value: Decimal, step: Decimal) -> Decimal {
    if step <= dec!(0) {
        return value;
    }
    (value / step).floor() * step
}

fn parse_decimal_path(spec: &Value, path: &[&str]) -> Option<Decimal> {
    let mut cur = spec;
    for key in path {
        cur = cur.get(*key)?;
    }
    match cur {
        Value::String(s) => Decimal::from_str(s).ok(),
        Value::Number(n) => Decimal::from_str(&n.to_string()).ok(),
        _ => None,
    }
}

fn max_spread_bps(spec: &Value) -> Option<Decimal> {
    spec.get("filters")?.as_array()?.iter().find_map(|f| {
        if f.get("kind").and_then(Value::as_str) == Some("max_spread_bps") {
            parse_decimal_path(f, &["params", "value"])
        } else {
            None
        }
    })
}

fn exit_param(spec: &Value, kind: &str, param: &str) -> Option<Decimal> {
    spec.get("exits")?.as_array()?.iter().find_map(|e| {
        if e.get("kind").and_then(Value::as_str) == Some(kind) {
            parse_decimal_path(e, &["params", param])
        } else {
            None
        }
    })
}

fn fmt(value: Decimal) -> String {
    value.round_dp(8).normalize().to_string()
}

fn default_maker_bps() -> Decimal {
    dec!(2)
}

fn default_taker_bps() -> Decimal {
    dec!(4)
}

fn default_slippage_bps() -> Decimal {
    dec!(1)
}

fn default_intrabar() -> IntrabarAssumption {
    IntrabarAssumption::Conservative
}

fn default_tick_size() -> Decimal {
    dec!(0.1)
}

fn default_lot_size() -> Decimal {
    dec!(0.001)
}

fn default_min_notional() -> Decimal {
    dec!(5)
}

fn default_max_leverage() -> Decimal {
    dec!(1)
}

fn default_maintenance_margin_bps() -> Decimal {
    dec!(50)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn spec() -> Value {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../packages/strategy-spec/fixtures/wyckoff_spring_btc_15m.json");
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
    }

    fn snapshot() -> MarketSnapshot {
        load_market_snapshot(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("fixtures/binance_btcusdt_15m_snapshot_2024_01_sample.json"),
        )
        .unwrap()
    }

    #[test]
    fn backtest_is_bit_identical_for_same_spec_and_snapshot() {
        let spec = spec();
        let snapshot = snapshot();
        let config = BacktestConfig {
            initial_equity: dec!(10000),
            data_snapshot_id: snapshot.data_snapshot_id.clone(),
            seed: 0,
            image_digest: "sha256:local-dev".to_string(),
        };
        let a = run_backtest(&spec, &snapshot, config.clone()).unwrap();
        let b = run_backtest(&spec, &snapshot, config).unwrap();
        assert_eq!(serde_json::to_string(&a).unwrap(), serde_json::to_string(&b).unwrap());
        assert_eq!(a.kpi_jsonb.meta.data_snapshot_id, snapshot.data_snapshot_id);
        assert!(!a.trades.is_empty());
    }
}
