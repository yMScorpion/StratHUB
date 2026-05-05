//! exec-rs — single executor binary for backtest, paper, and live modes.
//!
//! Phase 0 scope: load + validate a spec, log its mode and `spec_hash`. Phases 4-6 add the
//! interpreter, exchange adapters, risk guard, and order outbox.

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, ValueEnum};
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
        "spec loaded; interpreter not yet implemented"
    );

    // Phase 4+: dispatch to the interpreter wired to the appropriate data source / order sink.
    Ok(())
}
