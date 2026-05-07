use std::collections::{BTreeMap, HashMap};
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
    #[serde(
        default = "default_maintenance_margin_bps",
        with = "rust_decimal::serde::str"
    )]
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
    side: Side,
    qty: Decimal,
    entry_price: Decimal,
    stop_price: Decimal,
    take_profit_price: Decimal,
    risk_amount: Decimal,
    highest_price: Decimal,
    lowest_price: Decimal,
    trailing: Option<TrailingExit>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    Long,
    Short,
}

impl Side {
    fn from_str(value: &str) -> Option<Self> {
        match value {
            "long" => Some(Self::Long),
            "short" => Some(Self::Short),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Long => "long",
            Self::Short => "short",
        }
    }

    fn is_long(self) -> bool {
        matches!(self, Self::Long)
    }
}

#[derive(Debug, Clone)]
struct EntryRule {
    side: Side,
    when: Expr,
    size: SizeRule,
}

#[derive(Debug, Clone)]
struct SizeRule {
    kind: String,
    value: Decimal,
    kelly_cap: Option<Decimal>,
}

#[derive(Debug, Clone)]
struct ExitRules {
    stop: StopRule,
    take_profit: TakeProfitRule,
    trailing: Option<TrailingExit>,
}

#[derive(Debug, Clone)]
enum StopRule {
    Atr {
        indicator_id: Option<String>,
        mult: Decimal,
    },
    Percent(Decimal),
    Swing {
        lookback: usize,
    },
}

#[derive(Debug, Clone)]
enum TakeProfitRule {
    Rr(Decimal),
    Percent(Decimal),
    Atr {
        indicator_id: Option<String>,
        mult: Decimal,
    },
}

#[derive(Debug, Clone)]
struct TrailingExit {
    mode: String,
    indicator_id: Option<String>,
    mult: Decimal,
    lookback: usize,
}

#[derive(Debug, Clone)]
struct IndicatorSeries {
    values: HashMap<String, Vec<Option<Decimal>>>,
    atr_fallback: Vec<Option<Decimal>>,
    volume_ma_fallback: Vec<Option<Decimal>>,
}

pub fn load_market_snapshot(path: &Path) -> Result<MarketSnapshot> {
    let snapshot = match path.extension().and_then(|x| x.to_str()) {
        Some("json") => {
            let raw = std::fs::read(path)
                .with_context(|| format!("read market data {}", path.display()))?;
            serde_json::from_slice(&raw).context("parse market snapshot JSON")?
        }
        Some("duckdb") | Some("db") | Some("parquet") => load_duckdb_parquet_snapshot(path)?,
        other => anyhow::bail!(
            "unsupported market data format {:?}; expected .json, .duckdb, .db, or .parquet",
            other
        ),
    };
    anyhow::ensure!(
        snapshot.candles.len() >= 30,
        "market snapshot needs at least 30 candles for deterministic indicators"
    );
    Ok(snapshot)
}

fn load_duckdb_parquet_snapshot(path: &Path) -> Result<MarketSnapshot> {
    let is_parquet = path.extension().and_then(|x| x.to_str()) == Some("parquet");
    let conn = if is_parquet {
        duckdb::Connection::open_in_memory().context("open in-memory DuckDB")?
    } else {
        duckdb::Connection::open(path).with_context(|| format!("open DuckDB {}", path.display()))?
    };

    let relation = if is_parquet {
        format!("read_parquet('{}')", sql_quote(path))
    } else {
        "candles".to_string()
    };

    let mut snapshot = if is_parquet {
        parquet_snapshot_metadata(path, &conn, &relation)?
    } else {
        duckdb_snapshot_metadata(path, &conn)?
    };
    snapshot.candles = load_candles_from_relation(&conn, &relation)?;
    Ok(snapshot)
}

fn parquet_snapshot_metadata(
    path: &Path,
    conn: &duckdb::Connection,
    relation: &str,
) -> Result<MarketSnapshot> {
    let sql = format!("select data_snapshot_id, venue, symbol, timeframe from {relation} limit 1");
    let mut stmt = conn
        .prepare(&sql)
        .context("prepare parquet metadata query")?;
    let mut rows = stmt.query([]).context("query parquet metadata")?;
    let row = rows.next()?.context("parquet snapshot has no rows")?;
    Ok(MarketSnapshot {
        data_snapshot_id: row.get::<_, String>(0).unwrap_or_else(|_| file_stem(path)),
        venue: row.get::<_, String>(1).unwrap_or_default(),
        symbol: row.get::<_, String>(2).unwrap_or_default(),
        timeframe: row.get::<_, String>(3).unwrap_or_default(),
        fees: FeeModel::default(),
        slippage: SlippageModel::default(),
        funding: FundingModel::default(),
        contract: ContractSpec::default(),
        candles: Vec::new(),
    })
}

fn duckdb_snapshot_metadata(path: &Path, conn: &duckdb::Connection) -> Result<MarketSnapshot> {
    let sql = "select id, venue, symbol, timeframe from market_data_snapshots limit 1";
    let base = match conn.prepare(sql).and_then(|mut stmt| {
        let mut rows = stmt.query([])?;
        if let Some(row) = rows.next()? {
            Ok(Some((
                row.get::<_, String>(0).unwrap_or_else(|_| file_stem(path)),
                row.get::<_, String>(1).unwrap_or_default(),
                row.get::<_, String>(2).unwrap_or_default(),
                row.get::<_, String>(3).unwrap_or_default(),
            )))
        } else {
            Ok(None)
        }
    }) {
        Ok(Some(base)) => base,
        _ => (file_stem(path), String::new(), String::new(), String::new()),
    };

    Ok(MarketSnapshot {
        data_snapshot_id: base.0,
        venue: base.1,
        symbol: base.2,
        timeframe: base.3,
        fees: FeeModel::default(),
        slippage: SlippageModel::default(),
        funding: FundingModel::default(),
        contract: ContractSpec::default(),
        candles: Vec::new(),
    })
}

fn load_candles_from_relation(conn: &duckdb::Connection, relation: &str) -> Result<Vec<Candle>> {
    let sql = format!(
        "select ts, open, high, low, close, volume, spread_bps, coalesce(delisted, false) \
         from {relation} order by ts"
    );
    let mut stmt = conn.prepare(&sql).context("prepare candle query")?;
    let rows = stmt
        .query_map([], |row| {
            Ok(Candle {
                ts: row.get::<_, String>(0)?,
                open: decimal_from_duckdb(row, 1)?,
                high: decimal_from_duckdb(row, 2)?,
                low: decimal_from_duckdb(row, 3)?,
                close: decimal_from_duckdb(row, 4)?,
                volume: decimal_from_duckdb(row, 5)?,
                spread_bps: optional_decimal_from_duckdb(row, 6)?,
                delisted: row.get::<_, bool>(7).unwrap_or(false),
            })
        })
        .context("query candles")?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.context("decode candle row")?);
    }
    Ok(out)
}

fn decimal_from_duckdb(row: &duckdb::Row<'_>, idx: usize) -> duckdb::Result<Decimal> {
    let raw: duckdb::types::Value = row.get(idx)?;
    Ok(decimal_from_value(raw))
}

fn optional_decimal_from_duckdb(
    row: &duckdb::Row<'_>,
    idx: usize,
) -> duckdb::Result<Option<Decimal>> {
    let raw: duckdb::types::Value = row.get(idx)?;
    if matches!(raw, duckdb::types::Value::Null) {
        Ok(None)
    } else {
        Ok(Some(decimal_from_value(raw)))
    }
}

fn decimal_from_value(value: duckdb::types::Value) -> Decimal {
    match value {
        duckdb::types::Value::TinyInt(x) => Decimal::from(x),
        duckdb::types::Value::SmallInt(x) => Decimal::from(x),
        duckdb::types::Value::Int(x) => Decimal::from(x),
        duckdb::types::Value::BigInt(x) => Decimal::from(x),
        duckdb::types::Value::HugeInt(x) => Decimal::from_i128_with_scale(x, 0),
        duckdb::types::Value::UTinyInt(x) => Decimal::from(x),
        duckdb::types::Value::USmallInt(x) => Decimal::from(x),
        duckdb::types::Value::UInt(x) => Decimal::from(x),
        duckdb::types::Value::UBigInt(x) => Decimal::from(x),
        duckdb::types::Value::Float(x) => Decimal::from_f32_retain(x).unwrap_or(dec!(0)),
        duckdb::types::Value::Double(x) => Decimal::from_f64_retain(x).unwrap_or(dec!(0)),
        duckdb::types::Value::Decimal(x) => Decimal::from_str(&x.to_string()).unwrap_or(dec!(0)),
        duckdb::types::Value::Text(x) => Decimal::from_str(&x).unwrap_or(dec!(0)),
        _ => dec!(0),
    }
}

fn file_stem(path: &Path) -> String {
    path.file_stem()
        .and_then(|x| x.to_str())
        .unwrap_or("market-data-snapshot")
        .to_string()
}

fn sql_quote(path: &Path) -> String {
    path.to_string_lossy().replace('\'', "''")
}

pub fn run_backtest(
    spec: &Value,
    snapshot: &MarketSnapshot,
    config: BacktestConfig,
) -> Result<BacktestReport> {
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
    let min_rr = parse_decimal_path(spec, &["risk", "min_rr"]).unwrap_or(dec!(3));
    let indicators = build_indicator_series(spec, &snapshot.candles);
    let entries = parse_entries(spec)?;
    let exits = parse_exit_rules(spec, min_rr);
    let mut equity = config.initial_equity;
    let mut peak = equity;
    let mut position: Option<Position> = None;
    let mut trades = Vec::new();
    let mut equity_curve = Vec::new();

    for i in 1..snapshot.candles.len() {
        let c = &snapshot.candles[i];
        if c.delisted {
            if let Some(pos) = position.take() {
                let exit_buy = !pos.side.is_long();
                close_position(
                    &mut equity,
                    &mut trades,
                    ClosePosition {
                        symbol: &symbol,
                        pos,
                        candle: c,
                        exit_price: execution_price(c.close, snapshot.slippage.bps, exit_buy),
                        exit_reason: "delisting",
                        snapshot,
                    },
                );
            }
            push_equity(&mut equity_curve, c, equity, &mut peak);
            continue;
        }

        if let Some(mut pos) = position.take() {
            apply_trailing_stop(
                &mut pos,
                &snapshot.candles,
                &indicators,
                i,
                snapshot.contract.tick_size,
            );
            let hit_stop = if pos.side.is_long() {
                c.low <= pos.stop_price
            } else {
                c.high >= pos.stop_price
            };
            let hit_tp = if pos.side.is_long() {
                c.high >= pos.take_profit_price
            } else {
                c.low <= pos.take_profit_price
            };
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
                let exit_buy = !pos.side.is_long();
                close_position(
                    &mut equity,
                    &mut trades,
                    ClosePosition {
                        symbol: &symbol,
                        pos,
                        candle: c,
                        exit_price: execution_price(price, snapshot.slippage.bps, exit_buy),
                        exit_reason: reason,
                        snapshot,
                    },
                );
            } else {
                position = Some(pos);
            }
        }

        if position.is_none() {
            let context = eval_context(spec, &snapshot.candles, &indicators, i);
            for entry in &entries {
                if !entry.when.eval(&context) {
                    continue;
                }
                if let Some(next) = open_position(
                    entry,
                    &exits,
                    &snapshot.candles,
                    &indicators,
                    i,
                    equity,
                    snapshot,
                ) {
                    position = Some(next);
                    break;
                }
            }
        }

        push_equity(&mut equity_curve, c, equity, &mut peak);
    }

    if let Some(pos) = position.take() {
        let last = snapshot.candles.last().expect("snapshot has candles");
        let exit_buy = !pos.side.is_long();
        close_position(
            &mut equity,
            &mut trades,
            ClosePosition {
                symbol: &symbol,
                pos,
                candle: last,
                exit_price: execution_price(last.close, snapshot.slippage.bps, exit_buy),
                exit_reason: "end_of_data",
                snapshot,
            },
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

struct ClosePosition<'a> {
    symbol: &'a str,
    pos: Position,
    candle: &'a Candle,
    exit_price: Decimal,
    exit_reason: &'a str,
    snapshot: &'a MarketSnapshot,
}

fn close_position(equity: &mut Decimal, trades: &mut Vec<Trade>, close: ClosePosition<'_>) {
    let ClosePosition {
        symbol,
        pos,
        candle,
        exit_price,
        exit_reason,
        snapshot,
    } = close;
    let notional_in = pos.qty * pos.entry_price;
    let notional_out = pos.qty * exit_price;
    let gross_pnl = if pos.side.is_long() {
        (exit_price - pos.entry_price) * pos.qty
    } else {
        (pos.entry_price - exit_price) * pos.qty
    };
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
        side: pos.side.as_str().to_string(),
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
    lib_versions.insert("rust_decimal".to_string(), "1.41.0".to_string());
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

fn parse_entries(spec: &Value) -> Result<Vec<EntryRule>> {
    let mut out = Vec::new();
    for entry in spec
        .get("entries")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let side = entry
            .get("side")
            .and_then(Value::as_str)
            .and_then(Side::from_str)
            .context("entry side must be long or short")?;
        let when = entry
            .get("when")
            .and_then(Value::as_str)
            .context("entry.when is required")?;
        let size = entry.get("size").context("entry.size is required")?;
        out.push(EntryRule {
            side,
            when: ExprParser::new(when).parse()?,
            size: SizeRule {
                kind: size
                    .get("kind")
                    .and_then(Value::as_str)
                    .unwrap_or("risk_pct")
                    .to_string(),
                value: parse_decimal_path(size, &["value"]).unwrap_or(dec!(0.5)),
                kelly_cap: parse_decimal_path(size, &["kelly_cap"]),
            },
        });
    }
    anyhow::ensure!(!out.is_empty(), "at least one entry rule is required");
    Ok(out)
}

fn parse_exit_rules(spec: &Value, min_rr: Decimal) -> ExitRules {
    let stop = spec
        .get("exits")
        .and_then(Value::as_array)
        .and_then(|exits| {
            exits
                .iter()
                .find_map(|e| match e.get("kind").and_then(Value::as_str)? {
                    "stop_atr" => Some(StopRule::Atr {
                        indicator_id: e
                            .get("params")
                            .and_then(|p| p.get("atr_id"))
                            .and_then(Value::as_str)
                            .map(str::to_string),
                        mult: parse_decimal_path(e, &["params", "mult"]).unwrap_or(dec!(1.5)),
                    }),
                    "stop_pct" => Some(StopRule::Percent(
                        parse_decimal_path(e, &["params", "pct"]).unwrap_or(dec!(1)),
                    )),
                    "stop_swing" => Some(StopRule::Swing {
                        lookback: parse_usize_path(e, &["params", "lookback"]).unwrap_or(20),
                    }),
                    _ => None,
                })
        })
        .unwrap_or(StopRule::Atr {
            indicator_id: None,
            mult: dec!(1.5),
        });

    let take_profit = spec
        .get("exits")
        .and_then(Value::as_array)
        .and_then(|exits| {
            exits
                .iter()
                .find_map(|e| match e.get("kind").and_then(Value::as_str)? {
                    "tp_rr" => Some(TakeProfitRule::Rr(
                        parse_decimal_path(e, &["params", "rr"]).unwrap_or(min_rr),
                    )),
                    "tp_pct" => Some(TakeProfitRule::Percent(
                        parse_decimal_path(e, &["params", "pct"]).unwrap_or(dec!(3)),
                    )),
                    "tp_atr" => Some(TakeProfitRule::Atr {
                        indicator_id: e
                            .get("params")
                            .and_then(|p| p.get("atr_id"))
                            .and_then(Value::as_str)
                            .map(str::to_string),
                        mult: parse_decimal_path(e, &["params", "mult"]).unwrap_or(dec!(3)),
                    }),
                    _ => None,
                })
        })
        .unwrap_or(TakeProfitRule::Rr(min_rr));

    let trailing = spec
        .get("exits")
        .and_then(Value::as_array)
        .and_then(|exits| {
            exits.iter().find_map(|e| {
                if e.get("kind").and_then(Value::as_str) != Some("trailing") {
                    return None;
                }
                Some(TrailingExit {
                    mode: e
                        .get("params")
                        .and_then(|p| p.get("mode"))
                        .and_then(Value::as_str)
                        .unwrap_or("atr")
                        .to_string(),
                    indicator_id: e
                        .get("params")
                        .and_then(|p| p.get("atr_id").or_else(|| p.get("ref_id")))
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    mult: parse_decimal_path(e, &["params", "mult"]).unwrap_or(dec!(1)),
                    lookback: parse_usize_path(e, &["params", "lookback"]).unwrap_or(10),
                })
            })
        });

    ExitRules {
        stop,
        take_profit,
        trailing,
    }
}

fn open_position(
    entry: &EntryRule,
    exits: &ExitRules,
    candles: &[Candle],
    indicators: &IndicatorSeries,
    i: usize,
    equity: Decimal,
    snapshot: &MarketSnapshot,
) -> Option<Position> {
    let c = &candles[i];
    let entry_price = execution_price(c.close, snapshot.slippage.bps, entry.side.is_long());
    let stop_price = initial_stop(
        entry.side,
        entry_price,
        exits,
        candles,
        indicators,
        i,
        snapshot.contract.tick_size,
    )?;
    let risk_per_unit = if entry.side.is_long() {
        entry_price - stop_price
    } else {
        stop_price - entry_price
    };
    if risk_per_unit <= dec!(0) {
        return None;
    }

    let risk_amount = position_risk_amount(&entry.size, equity, entry_price, risk_per_unit);
    let qty = round_down(risk_amount / risk_per_unit, snapshot.contract.lot_size);
    if qty <= dec!(0) || qty * entry_price < snapshot.contract.min_notional {
        return None;
    }

    let take_profit_price = initial_take_profit(
        entry.side,
        entry_price,
        stop_price,
        exits,
        indicators,
        i,
        snapshot.contract.tick_size,
    )?;
    Some(Position {
        entry_ts: c.ts.clone(),
        side: entry.side,
        qty,
        entry_price,
        stop_price,
        take_profit_price,
        risk_amount,
        highest_price: c.high,
        lowest_price: c.low,
        trailing: exits.trailing.clone(),
    })
}

fn position_risk_amount(
    size: &SizeRule,
    equity: Decimal,
    entry_price: Decimal,
    risk_per_unit: Decimal,
) -> Decimal {
    match size.kind.as_str() {
        "fixed_pct" => equity * (size.value / dec!(100)) * (risk_per_unit / entry_price),
        "fixed_notional" => size.value * (risk_per_unit / entry_price),
        "kelly_fraction" => {
            equity
                * size.value.min(size.kelly_cap.unwrap_or(size.value))
                * (risk_per_unit / entry_price)
        }
        "vol_target" => equity * (size.value / dec!(100)) * (risk_per_unit / entry_price),
        _ => equity * (size.value / dec!(100)),
    }
}

fn initial_stop(
    side: Side,
    entry_price: Decimal,
    exits: &ExitRules,
    candles: &[Candle],
    indicators: &IndicatorSeries,
    i: usize,
    tick_size: Decimal,
) -> Option<Decimal> {
    let raw = match &exits.stop {
        StopRule::Atr { indicator_id, mult } => {
            let atr = indicator_value(indicators, indicator_id.as_deref(), i)?;
            if side.is_long() {
                entry_price - atr * *mult
            } else {
                entry_price + atr * *mult
            }
        }
        StopRule::Percent(pct) => {
            if side.is_long() {
                entry_price * (dec!(1) - *pct / dec!(100))
            } else {
                entry_price * (dec!(1) + *pct / dec!(100))
            }
        }
        StopRule::Swing { lookback } => swing_stop(side, candles, i, *lookback)?,
    };
    Some(if side.is_long() {
        round_down(raw, tick_size)
    } else {
        round_up(raw, tick_size)
    })
}

fn initial_take_profit(
    side: Side,
    entry_price: Decimal,
    stop_price: Decimal,
    exits: &ExitRules,
    indicators: &IndicatorSeries,
    i: usize,
    tick_size: Decimal,
) -> Option<Decimal> {
    let risk = if side.is_long() {
        entry_price - stop_price
    } else {
        stop_price - entry_price
    };
    let raw = match &exits.take_profit {
        TakeProfitRule::Rr(rr) => {
            if side.is_long() {
                entry_price + risk * *rr
            } else {
                entry_price - risk * *rr
            }
        }
        TakeProfitRule::Percent(pct) => {
            if side.is_long() {
                entry_price * (dec!(1) + *pct / dec!(100))
            } else {
                entry_price * (dec!(1) - *pct / dec!(100))
            }
        }
        TakeProfitRule::Atr { indicator_id, mult } => {
            let atr = indicator_value(indicators, indicator_id.as_deref(), i)?;
            if side.is_long() {
                entry_price + atr * *mult
            } else {
                entry_price - atr * *mult
            }
        }
    };
    Some(if side.is_long() {
        round_down(raw, tick_size)
    } else {
        round_up(raw, tick_size)
    })
}

fn apply_trailing_stop(
    pos: &mut Position,
    candles: &[Candle],
    indicators: &IndicatorSeries,
    i: usize,
    tick_size: Decimal,
) {
    let Some(trailing) = pos.trailing.clone() else {
        return;
    };
    let c = &candles[i];
    pos.highest_price = pos.highest_price.max(c.high);
    pos.lowest_price = pos.lowest_price.min(c.low);
    let candidate = match trailing.mode.as_str() {
        "atr" => indicator_value(indicators, trailing.indicator_id.as_deref(), i).map(|atr| {
            if pos.side.is_long() {
                pos.highest_price - atr * trailing.mult
            } else {
                pos.lowest_price + atr * trailing.mult
            }
        }),
        "swing" | "order_block" => swing_stop(pos.side, candles, i, trailing.lookback),
        _ => None,
    };
    if let Some(raw) = candidate {
        if pos.side.is_long() {
            pos.stop_price = pos.stop_price.max(round_down(raw, tick_size));
        } else {
            pos.stop_price = pos.stop_price.min(round_up(raw, tick_size));
        }
    }
}

fn swing_stop(side: Side, candles: &[Candle], i: usize, lookback: usize) -> Option<Decimal> {
    if i == 0 {
        return None;
    }
    let start = i.saturating_sub(lookback).max(1);
    if side.is_long() {
        candles[start..i].iter().map(|c| c.low).min()
    } else {
        candles[start..i].iter().map(|c| c.high).max()
    }
}

fn indicator_value(indicators: &IndicatorSeries, id: Option<&str>, i: usize) -> Option<Decimal> {
    id.and_then(|key| indicators.values.get(key))
        .and_then(|xs| xs.get(i).copied().flatten())
        .or_else(|| indicators.atr_fallback.get(i).copied().flatten())
}

fn build_indicator_series(spec: &Value, candles: &[Candle]) -> IndicatorSeries {
    let atr_fallback = atr(candles, 14);
    let volume_ma_fallback = volume_ma(candles, 20);
    let mut values = HashMap::new();
    for indicator in spec
        .get("indicators")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(id) = indicator.get("id").and_then(Value::as_str) else {
            continue;
        };
        let length = parse_usize_path(indicator, &["params", "length"]).unwrap_or(14);
        let source = indicator
            .get("source")
            .and_then(Value::as_str)
            .unwrap_or("close");
        let series = match indicator.get("kind").and_then(Value::as_str).unwrap_or("") {
            "atr" => atr(candles, length),
            "volume_ma" => volume_ma(candles, length),
            "sma" => moving_average(candles, length, source),
            "ema" => ema(candles, length, source),
            "rsi" => rsi(candles, length),
            _ => vec![None; candles.len()],
        };
        values.insert(id.to_string(), series);
    }
    IndicatorSeries {
        values,
        atr_fallback,
        volume_ma_fallback,
    }
}

fn eval_context(
    spec: &Value,
    candles: &[Candle],
    indicators: &IndicatorSeries,
    i: usize,
) -> HashMap<String, bool> {
    let mut out = HashMap::new();
    for indicator in spec
        .get("indicators")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if let Some(id) = indicator.get("id").and_then(Value::as_str) {
            out.insert(
                id.to_string(),
                indicators
                    .values
                    .get(id)
                    .and_then(|xs| xs.get(i))
                    .and_then(|x| *x)
                    .is_some(),
            );
        }
    }
    for pattern in spec
        .get("patterns")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(id) = pattern.get("id").and_then(Value::as_str) else {
            continue;
        };
        out.insert(
            id.to_string(),
            eval_pattern(pattern, candles, indicators, i),
        );
    }
    for (idx, filter) in spec
        .get("filters")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .enumerate()
    {
        let name = filter
            .get("kind")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| format!("filter_{idx}"));
        let ok = eval_filter(filter, candles, indicators, i);
        out.insert(name, ok);
    }
    out
}

fn eval_pattern(
    pattern: &Value,
    candles: &[Candle],
    indicators: &IndicatorSeries,
    i: usize,
) -> bool {
    match pattern.get("kind").and_then(Value::as_str).unwrap_or("") {
        "wyckoff_spring" => {
            let lookback = parse_usize_path(pattern, &["params", "lookback"]).unwrap_or(20);
            let penetration =
                parse_decimal_path(pattern, &["params", "penetration_atr"]).unwrap_or(dec!(0.5));
            wyckoff_spring(candles, &indicators.atr_fallback, i, lookback, penetration)
        }
        "wyckoff_upthrust" => {
            let lookback = parse_usize_path(pattern, &["params", "lookback"]).unwrap_or(20);
            wyckoff_upthrust(candles, &indicators.atr_fallback, i, lookback)
        }
        "vsa_no_supply" => {
            let ratio =
                parse_decimal_path(pattern, &["params", "vol_ratio_max"]).unwrap_or(dec!(0.7));
            vsa_no_supply(candles, &indicators.volume_ma_fallback, i, ratio)
        }
        "vsa_no_demand" => {
            let ratio =
                parse_decimal_path(pattern, &["params", "vol_ratio_max"]).unwrap_or(dec!(0.7));
            vsa_no_demand(candles, &indicators.volume_ma_fallback, i, ratio)
        }
        "vsa_stopping_volume" | "vsa_climactic_volume" => {
            high_volume_bar(candles, &indicators.volume_ma_fallback, i)
        }
        "vsa_effort_no_result" | "vsa_test_bar" => {
            narrow_spread_low_volume(candles, &indicators.volume_ma_fallback, i)
        }
        "order_block" => order_block(pattern, candles, i),
        "liquidity_sweep" => {
            wyckoff_spring(candles, &indicators.atr_fallback, i, 20, dec!(1))
                || wyckoff_upthrust(candles, &indicators.atr_fallback, i, 20)
        }
        "break_of_structure" => {
            i >= 5
                && candles[i].close
                    > candles[i - 5..i]
                        .iter()
                        .map(|c| c.high)
                        .max()
                        .unwrap_or(candles[i].high)
        }
        "change_of_character" => {
            i >= 5
                && candles[i].close
                    < candles[i - 5..i]
                        .iter()
                        .map(|c| c.low)
                        .min()
                        .unwrap_or(candles[i].low)
        }
        _ => false,
    }
}

fn eval_filter(filter: &Value, candles: &[Candle], indicators: &IndicatorSeries, i: usize) -> bool {
    let c = &candles[i];
    match filter.get("kind").and_then(Value::as_str).unwrap_or("") {
        "max_spread_bps" => {
            c.spread_bps.unwrap_or_default()
                <= parse_decimal_path(filter, &["params", "value"]).unwrap_or(dec!(999999))
        }
        "min_price" => {
            c.close >= parse_decimal_path(filter, &["params", "value"]).unwrap_or(dec!(0))
        }
        "session" => in_session(
            &c.ts,
            filter
                .get("params")
                .and_then(|p| p.get("window"))
                .and_then(Value::as_str)
                .unwrap_or("00-23"),
        ),
        "weekday" => true,
        "volatility" => {
            let Some(atr) = indicators.atr_fallback.get(i).copied().flatten() else {
                return false;
            };
            let pct = atr / c.close * dec!(100);
            let min = parse_decimal_path(filter, &["params", "min_pct"]).unwrap_or(dec!(0));
            let max = parse_decimal_path(filter, &["params", "max_pct"]).unwrap_or(dec!(999999));
            pct >= min && pct <= max
        }
        "regime" => true,
        _ => false,
    }
}

fn in_session(ts: &str, window: &str) -> bool {
    let hour = ts
        .get(11..13)
        .and_then(|h| h.parse::<u32>().ok())
        .unwrap_or(0);
    let mut parts = window.split('-');
    let start = parts
        .next()
        .and_then(|x| x.parse::<u32>().ok())
        .unwrap_or(0);
    let end = parts
        .next()
        .and_then(|x| x.parse::<u32>().ok())
        .unwrap_or(23);
    if start <= end {
        hour >= start && hour <= end
    } else {
        hour >= start || hour <= end
    }
}

fn atr(candles: &[Candle], length: usize) -> Vec<Option<Decimal>> {
    let mut true_ranges = vec![dec!(0); candles.len()];
    for i in 1..candles.len() {
        let c = &candles[i];
        let prev_close = candles[i - 1].close;
        true_ranges[i] = (c.high - c.low)
            .max((c.high - prev_close).abs())
            .max((c.low - prev_close).abs());
    }
    let mut out = vec![None; candles.len()];
    if length == 0 {
        return out;
    }
    for i in length..candles.len() {
        let sum: Decimal = true_ranges[i + 1 - length..=i].iter().sum();
        out[i] = Some(sum / Decimal::from(length as u64));
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

fn wyckoff_spring(
    candles: &[Candle],
    atr: &[Option<Decimal>],
    i: usize,
    lookback: usize,
    penetration_atr: Decimal,
) -> bool {
    let effective_lookback = lookback.min(i);
    if effective_lookback < 2 {
        return false;
    }
    let c = &candles[i];
    let prior_low = candles[i - effective_lookback..i]
        .iter()
        .map(|x| x.low)
        .min()
        .unwrap_or(c.low);
    let Some(atr) = atr[i] else {
        return false;
    };
    c.low < prior_low && c.close > prior_low && (prior_low - c.low) <= atr * penetration_atr
}

fn wyckoff_upthrust(
    candles: &[Candle],
    atr: &[Option<Decimal>],
    i: usize,
    lookback: usize,
) -> bool {
    let effective_lookback = lookback.min(i);
    if effective_lookback < 2 {
        return false;
    }
    let c = &candles[i];
    let prior_high = candles[i - effective_lookback..i]
        .iter()
        .map(|x| x.high)
        .max()
        .unwrap_or(c.high);
    let Some(atr) = atr[i] else {
        return false;
    };
    c.high > prior_high && c.close < prior_high && (c.high - prior_high) <= atr
}

fn vsa_no_supply(
    candles: &[Candle],
    volume_ma: &[Option<Decimal>],
    i: usize,
    vol_ratio_max: Decimal,
) -> bool {
    let c = &candles[i];
    let Some(ma) = volume_ma[i] else {
        return false;
    };
    c.close < c.open && c.volume <= ma * vol_ratio_max
}

fn vsa_no_demand(
    candles: &[Candle],
    volume_ma: &[Option<Decimal>],
    i: usize,
    vol_ratio_max: Decimal,
) -> bool {
    let c = &candles[i];
    let Some(ma) = volume_ma[i] else {
        return false;
    };
    c.close > c.open && c.volume <= ma * vol_ratio_max
}

fn high_volume_bar(candles: &[Candle], volume_ma: &[Option<Decimal>], i: usize) -> bool {
    volume_ma[i].is_some_and(|ma| candles[i].volume >= ma * dec!(1.5))
}

fn narrow_spread_low_volume(candles: &[Candle], volume_ma: &[Option<Decimal>], i: usize) -> bool {
    if i == 0 {
        return false;
    }
    let spread = candles[i].high - candles[i].low;
    let prev_spread = candles[i - 1].high - candles[i - 1].low;
    volume_ma[i].is_some_and(|ma| spread <= prev_spread && candles[i].volume <= ma)
}

fn order_block(pattern: &Value, candles: &[Candle], i: usize) -> bool {
    if i == 0 {
        return false;
    }
    let side = pattern
        .get("params")
        .and_then(|p| p.get("side"))
        .and_then(Value::as_str)
        .unwrap_or("demand");
    let prev = &candles[i - 1];
    let c = &candles[i];
    match side {
        "supply" => prev.close > prev.open && c.close < prev.low,
        _ => prev.close < prev.open && c.close > prev.high,
    }
}

fn moving_average(candles: &[Candle], length: usize, source: &str) -> Vec<Option<Decimal>> {
    let mut out = vec![None; candles.len()];
    if length == 0 {
        return out;
    }
    for i in length..candles.len() {
        let sum: Decimal = candles[i + 1 - length..=i]
            .iter()
            .map(|c| candle_source(c, source))
            .sum();
        out[i] = Some(sum / Decimal::from(length as u64));
    }
    out
}

fn ema(candles: &[Candle], length: usize, source: &str) -> Vec<Option<Decimal>> {
    let mut out = vec![None; candles.len()];
    if length == 0 || candles.is_empty() {
        return out;
    }
    let k = dec!(2) / Decimal::from((length + 1) as u64);
    let mut prev = candle_source(&candles[0], source);
    for (i, candle) in candles.iter().enumerate() {
        let value = candle_source(candle, source);
        prev = value * k + prev * (dec!(1) - k);
        if i >= length {
            out[i] = Some(prev);
        }
    }
    out
}

fn rsi(candles: &[Candle], length: usize) -> Vec<Option<Decimal>> {
    let mut out = vec![None; candles.len()];
    if length == 0 || candles.len() <= length {
        return out;
    }
    for i in length..candles.len() {
        let mut gains = dec!(0);
        let mut losses = dec!(0);
        for pair in candles[i + 1 - length..=i].windows(2) {
            let delta = pair[1].close - pair[0].close;
            if delta >= dec!(0) {
                gains += delta;
            } else {
                losses += delta.abs();
            }
        }
        out[i] = Some(if losses == dec!(0) {
            dec!(100)
        } else {
            dec!(100) - dec!(100) / (dec!(1) + gains / losses)
        });
    }
    out
}

fn candle_source(c: &Candle, source: &str) -> Decimal {
    match source {
        "open" => c.open,
        "high" => c.high,
        "low" => c.low,
        "hlc3" => (c.high + c.low + c.close) / dec!(3),
        "ohlc4" => (c.open + c.high + c.low + c.close) / dec!(4),
        "volume" => c.volume,
        _ => c.close,
    }
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

fn round_up(value: Decimal, step: Decimal) -> Decimal {
    if step <= dec!(0) {
        return value;
    }
    (value / step).ceil() * step
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

fn parse_usize_path(spec: &Value, path: &[&str]) -> Option<usize> {
    let mut cur = spec;
    for key in path {
        cur = cur.get(*key)?;
    }
    match cur {
        Value::Number(n) => n.as_u64().map(|x| x as usize),
        Value::String(s) => s.parse().ok(),
        _ => None,
    }
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

#[derive(Debug, Clone)]
enum Expr {
    Id(String),
    Not(Box<Expr>),
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
}

impl Expr {
    fn eval(&self, values: &HashMap<String, bool>) -> bool {
        match self {
            Self::Id(id) => values.get(id).copied().unwrap_or(false),
            Self::Not(expr) => !expr.eval(values),
            Self::And(lhs, rhs) => lhs.eval(values) && rhs.eval(values),
            Self::Or(lhs, rhs) => lhs.eval(values) || rhs.eval(values),
        }
    }
}

struct ExprParser<'a> {
    input: &'a str,
    pos: usize,
}

impl<'a> ExprParser<'a> {
    fn new(input: &'a str) -> Self {
        Self { input, pos: 0 }
    }

    fn parse(mut self) -> Result<Expr> {
        let expr = self.parse_or()?;
        self.skip_ws();
        anyhow::ensure!(
            self.pos == self.input.len(),
            "unexpected token in entry.when"
        );
        Ok(expr)
    }

    fn parse_or(&mut self) -> Result<Expr> {
        let mut expr = self.parse_and()?;
        loop {
            self.skip_ws();
            if !self.consume("||") {
                break;
            }
            expr = Expr::Or(Box::new(expr), Box::new(self.parse_and()?));
        }
        Ok(expr)
    }

    fn parse_and(&mut self) -> Result<Expr> {
        let mut expr = self.parse_unary()?;
        loop {
            self.skip_ws();
            if !self.consume("&&") {
                break;
            }
            expr = Expr::And(Box::new(expr), Box::new(self.parse_unary()?));
        }
        Ok(expr)
    }

    fn parse_unary(&mut self) -> Result<Expr> {
        self.skip_ws();
        if self.consume("!") {
            return Ok(Expr::Not(Box::new(self.parse_unary()?)));
        }
        if self.consume("(") {
            let expr = self.parse_or()?;
            self.skip_ws();
            anyhow::ensure!(self.consume(")"), "missing closing ')' in entry.when");
            return Ok(expr);
        }
        self.parse_id()
    }

    fn parse_id(&mut self) -> Result<Expr> {
        self.skip_ws();
        let start = self.pos;
        while let Some(ch) = self.input[self.pos..].chars().next() {
            if ch.is_ascii_alphanumeric() || ch == '_' {
                self.pos += ch.len_utf8();
            } else {
                break;
            }
        }
        anyhow::ensure!(self.pos > start, "expected identifier in entry.when");
        Ok(Expr::Id(self.input[start..self.pos].to_string()))
    }

    fn skip_ws(&mut self) {
        while let Some(ch) = self.input[self.pos..].chars().next() {
            if ch.is_whitespace() {
                self.pos += ch.len_utf8();
            } else {
                break;
            }
        }
    }

    fn consume(&mut self, expected: &str) -> bool {
        if self.input[self.pos..].starts_with(expected) {
            self.pos += expected.len();
            true
        } else {
            false
        }
    }
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
    fn parquet_market_snapshot_loader_reads_duckdb_snapshot() {
        let path = std::env::temp_dir().join(format!(
            "exec-rs-parquet-loader-{}.parquet",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let conn = duckdb::Connection::open_in_memory().unwrap();
        conn.execute_batch(&format!(
            "copy (
                select
                  'parquet-snapshot-v1' as data_snapshot_id,
                  'binance' as venue,
                  'BTCUSDT' as symbol,
                  '15m' as timeframe,
                  strftime(timestamp '2024-01-01 00:00:00' + i * interval 15 minute, '%Y-%m-%dT%H:%M:%SZ') as ts,
                  100 + i as open,
                  101 + i as high,
                  99 + i as low,
                  100.5 + i as close,
                  1000 + i as volume,
                  1.0 as spread_bps,
                  false as delisted
                from range(30) t(i)
              ) to '{}' (format parquet)",
            sql_quote(&path)
        ))
        .unwrap();

        let snapshot = load_market_snapshot(&path).unwrap();
        assert_eq!(snapshot.data_snapshot_id, "parquet-snapshot-v1");
        assert_eq!(snapshot.candles.len(), 30);
        assert_eq!(snapshot.candles[0].ts, "2024-01-01T00:00:00Z");
        let _ = std::fs::remove_file(path);
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
        assert_eq!(
            serde_json::to_string(&a).unwrap(),
            serde_json::to_string(&b).unwrap()
        );
        assert_eq!(a.kpi_jsonb.meta.data_snapshot_id, snapshot.data_snapshot_id);
        assert!(!a.trades.is_empty());
    }
}
