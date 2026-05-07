//! exec-rs — single executor binary for backtest, paper, and live modes.
//!
//! Phase 4 scope: backtest mode uses the same validated Strategy Spec that paper/live modes will
//! consume, with deterministic decimal accounting over a snapshot-versioned market data file.

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, ValueEnum};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde_json::Value;
use strategy_spec::{hash_spec, semantic_check, validate};
use tracing_subscriber::EnvFilter;

mod backtest;

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

    /// Snapshot-versioned market data file for backtests.
    #[arg(long, env = "EXEC_MARKET_DATA")]
    market_data: Option<PathBuf>,

    /// Required snapshot id for reproducible backtests.
    #[arg(long, env = "EXEC_DATA_SNAPSHOT_ID")]
    data_snapshot_id: Option<String>,

    /// Starting equity for backtests.
    #[arg(long, default_value = "10000", env = "EXEC_INITIAL_EQUITY")]
    initial_equity: Decimal,

    /// Deterministic seed recorded into KPI metadata.
    #[arg(long, default_value_t = 0, env = "EXEC_SEED")]
    seed: u64,

    /// Container/image digest recorded into KPI metadata.
    #[arg(long, default_value = "sha256:local-dev", env = "EXEC_IMAGE_DIGEST")]
    image_digest: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .json()
        .with_writer(std::io::stderr)
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
            let market_data = args
                .market_data
                .as_deref()
                .context("--market-data is required in backtest mode")?;
            let snapshot = backtest::load_market_snapshot(market_data)?;
            let data_snapshot_id = args
                .data_snapshot_id
                .unwrap_or_else(|| snapshot.data_snapshot_id.clone());
            let report = backtest::run_backtest(
                &spec,
                &snapshot,
                backtest::BacktestConfig {
                    initial_equity: if args.initial_equity > dec!(0) {
                        args.initial_equity
                    } else {
                        anyhow::bail!("--initial-equity must be positive");
                    },
                    data_snapshot_id,
                    seed: args.seed,
                    image_digest: args.image_digest,
                },
            )?;
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        Mode::Paper | Mode::Live => {
            anyhow::bail!("{} mode is gated for Phase 5/6", args.mode.as_str());
        }
    }

    Ok(())
}
