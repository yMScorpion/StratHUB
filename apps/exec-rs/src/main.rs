//! exec-rs — single executor binary for backtest, paper, and live modes.
//!
//! Paper mode uses live market-data inputs with virtual execution. Exchange testnet is reserved
//! for adapter smoke tests, not strategy validation.

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, ValueEnum};
use exec_rs::{
    evaluate_scorecard, ClockSkewMonitor, Exchange, RiskGuard, RiskLimits, RiskState,
    ScorecardThresholds, ValidationMetrics,
};
use rust_decimal_macros::dec;
use serde_json::Value;
use strategy_spec::{hash_spec, semantic_check, validate};
use tracing_subscriber::EnvFilter;

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Mode {
    Backtest,
    Paper,
    Live,
}

impl Mode {
    fn as_str(self) -> &'static str {
        match self {
            Mode::Backtest => "backtest",
            Mode::Paper => "paper",
            Mode::Live => "live",
        }
    }
}

#[derive(Debug, Parser)]
#[command(name = "exec-rs", version, about = "Strategy Spec executor")]
struct Args {
    /// Execution mode.
    #[arg(long, value_enum)]
    mode: Mode,

    /// Path to a Strategy Spec JSON file.
    #[arg(long)]
    spec: PathBuf,

    /// Account id (required for paper/live).
    #[arg(long, env = "EXEC_ACCOUNT_ID")]
    account_id: Option<String>,

    /// Optional run id for tracing/correlation. Auto-generated if omitted.
    #[arg(long, env = "EXEC_RUN_ID")]
    run_id: Option<String>,

    /// Exchange venue used for paper/live connectivity.
    #[arg(long, value_enum, default_value = "binance", env = "EXEC_EXCHANGE")]
    exchange: ExchangeArg,

    /// In paper mode, emit a synthetic 7-day completion for validation smoke tests.
    #[arg(long, env = "EXEC_VALIDATION_SMOKE")]
    validation_smoke: bool,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ExchangeArg {
    Binance,
    Bybit,
}

impl From<ExchangeArg> for Exchange {
    fn from(value: ExchangeArg) -> Self {
        match value {
            ExchangeArg::Binance => Exchange::Binance,
            ExchangeArg::Bybit => Exchange::Bybit,
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let args = Args::parse();

    if matches!(args.mode, Mode::Paper | Mode::Live) && args.account_id.is_none() {
        anyhow::bail!("--account-id is required in paper and live modes");
    }

    let raw =
        std::fs::read(&args.spec).with_context(|| format!("read spec {}", args.spec.display()))?;
    let spec: Value = serde_json::from_slice(&raw).context("parse spec JSON")?;

    validate(&spec).context("schema validation")?;
    semantic_check(&spec).context("semantic validation")?;

    let computed = hash_spec(&spec);
    if let Some(declared) = spec.get("spec_hash").and_then(Value::as_str) {
        if declared != computed {
            anyhow::bail!("spec_hash mismatch: declared {declared}, computed {computed}");
        }
    }

    let run_id = args
        .run_id
        .unwrap_or_else(|| format!("run-{}", &computed[..16]));

    tracing::info!(
        mode = args.mode.as_str(),
        spec_hash = %computed,
        run_id = %run_id,
        account_id = args.account_id.as_deref().unwrap_or("-"),
        "spec loaded"
    );

    match args.mode {
        Mode::Backtest => {
            tracing::info!("backtest dispatch placeholder; phase 4 interpreter consumes this path");
        }
        Mode::Paper => {
            run_paper_validation(
                &computed,
                &run_id,
                args.exchange.into(),
                args.validation_smoke,
            )
            .await?;
        }
        Mode::Live => {
            tracing::info!("live dispatch guarded until phase 6 capital controls are enabled");
        }
    }
    Ok(())
}

async fn run_paper_validation(
    spec_hash: &str,
    run_id: &str,
    exchange: Exchange,
    validation_smoke: bool,
) -> Result<()> {
    let clock = ClockSkewMonitor::new(500);
    clock
        .observe(chrono::Utc::now(), chrono::Utc::now())
        .map_err(|alert| anyhow::anyhow!("clock skew check failed: {alert:?}"))?;

    let guard = RiskGuard::new(RiskLimits {
        kill_switch_active: false,
        max_daily_loss_pct: dec!(2),
        max_position_pct: dec!(5),
        max_concurrent_orders: 5,
    });
    guard
        .check(&RiskState {
            equity: dec!(10000),
            start_of_day_equity: dec!(10000),
            gross_position_pct: dec!(0),
            open_orders: 0,
        })
        .map_err(|alert| anyhow::anyhow!("risk guard blocked paper run: {alert:?}"))?;

    let thresholds = ScorecardThresholds {
        min_trades: 10,
        max_drawdown_pct: dec!(8),
        max_slippage_bps_vs_backtest: dec!(25),
        max_risk_guard_events: 0,
        max_exec_errors: 0,
        require_walk_forward: true,
        require_oos: true,
    };
    let metrics = if validation_smoke {
        ValidationMetrics {
            trades: 12,
            max_drawdown_pct: dec!(3.2),
            slippage_bps_vs_backtest: dec!(8),
            risk_guard_events: 0,
            exec_errors: 0,
            walk_forward_passed: true,
            oos_passed: true,
            run_days: 7,
        }
    } else {
        ValidationMetrics {
            trades: 0,
            max_drawdown_pct: dec!(0),
            slippage_bps_vs_backtest: dec!(0),
            risk_guard_events: 0,
            exec_errors: 0,
            walk_forward_passed: false,
            oos_passed: false,
            run_days: 0,
        }
    };
    let scorecard = evaluate_scorecard(&thresholds, &metrics);
    tracing::info!(
        spec_hash = %spec_hash,
        run_id = %run_id,
        exchange = ?exchange,
        region = exchange.primary_region(),
        paper_mode = true,
        strategy_testnet = false,
        scorecard_passed = scorecard.passed,
        scorecard_reasons = ?scorecard.reasons,
        "paper validation state"
    );
    Ok(())
}
