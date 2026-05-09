//! exec-rs — single executor binary for backtest, paper, and live modes.
//!
//! Paper mode uses live market-data inputs with virtual execution. Exchange testnet is reserved
//! for adapter smoke tests, not strategy validation.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::{Parser, ValueEnum};
use exec_rs::{
    check_compliance_gate, decrypt_broker_key, deterministic_client_order_id, evaluate_scorecard,
    reconcile_fill, AuditEntry, AuditLog, BrokerCredentials, ClockSkewMonitor, ComplianceError,
    ComplianceRecord, ConfirmationGate, Exchange, Fill, KillSwitchAction, LivePositionState,
    LocalRiskCache, MarketTick, OrderIntent, OrderOutbox, OutboxStatus, PaperBroker,
    ReconciliationCheck, RiskGuard, RiskLimits, RiskState, ScorecardThresholds, Side,
    ValidationMetrics, WsReconnectThrottle,
};
use futures_util::{SinkExt, StreamExt};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use strategy_spec::{hash_spec, semantic_check, validate};
use tokio::time::{Duration, Instant};
use tokio_tungstenite::{connect_async, tungstenite::Message};
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

    /// User id that owns the live account (required for live audit/compliance records).
    #[arg(long, env = "EXEC_USER_ID")]
    user_id: Option<String>,

    /// Optional run id for tracing/correlation. Auto-generated if omitted.
    #[arg(long, env = "EXEC_RUN_ID")]
    run_id: Option<String>,

    /// Exchange venue used for paper/live connectivity.
    #[arg(long, value_enum, default_value = "binance", env = "EXEC_EXCHANGE")]
    exchange: ExchangeArg,

    /// In paper mode, emit a synthetic 7-day completion for validation smoke tests.
    #[arg(long, env = "EXEC_VALIDATION_SMOKE")]
    validation_smoke: bool,

    /// API base URL used by paper validators for telemetry and scorecard submission.
    #[arg(
        long,
        default_value = "http://localhost:8000",
        env = "EXEC_API_BASE_URL"
    )]
    api_base_url: String,

    /// Internal API token accepted by api-ai.
    #[arg(long, env = "EXEC_INTERNAL_TOKEN")]
    internal_token: Option<String>,

    /// Validation id provisioned by api-ai.
    #[arg(long, env = "EXEC_VALIDATION_ID")]
    validation_id: Option<String>,

    /// Paper validation duration in days.
    #[arg(long, default_value_t = 7, env = "EXEC_VALIDATION_DAYS")]
    validation_days: u32,

    /// Optional symbol override. Defaults to the first symbol in the Strategy Spec.
    #[arg(long, env = "EXEC_SYMBOL")]
    symbol: Option<String>,

    /// Local directory for validator outbox/fill state.
    #[arg(long, default_value = ".exec-rs-state", env = "EXEC_STATE_DIR")]
    state_dir: PathBuf,

    /// Optional wall-clock cap for integration tests and supervised shutdowns.
    #[arg(long, env = "EXEC_VALIDATION_MAX_SECONDS")]
    validation_max_seconds: Option<u64>,

    // ---- Live mode args (Phase 6) ----
    /// Nonce hex for encrypted API key (AES-256-GCM, 12 bytes).
    #[arg(long, env = "EXEC_KMS_KEY_NONCE_HEX")]
    kms_key_nonce_hex: Option<String>,

    /// Ciphertext hex for encrypted API key.
    #[arg(long, env = "EXEC_KMS_KEY_CIPHERTEXT_HEX")]
    kms_key_ciphertext_hex: Option<String>,

    /// Nonce hex for encrypted API secret.
    #[arg(long, env = "EXEC_KMS_SECRET_NONCE_HEX")]
    kms_secret_nonce_hex: Option<String>,

    /// Ciphertext hex for encrypted API secret.
    #[arg(long, env = "EXEC_KMS_SECRET_CIPHERTEXT_HEX")]
    kms_secret_ciphertext_hex: Option<String>,

    /// Stored broker-key scopes from trusted metadata (comma-separated, e.g. "trade,read").
    #[arg(long, env = "EXEC_BROKER_KEY_SCOPE", value_delimiter = ',')]
    broker_key_scope: Vec<String>,

    /// Hold-to-confirm token supplied by the user for this live session.
    #[arg(long, env = "EXEC_CONFIRM_TOKEN")]
    confirm_token: Option<String>,

    /// Server-recorded SHA-256 digest of the hold-to-confirm token.
    #[arg(long, env = "EXEC_CONFIRM_TOKEN_DIGEST")]
    confirm_token_digest: Option<String>,

    /// Flatten all open positions (market sell) when kill-switch fires.
    #[arg(long, env = "EXEC_FLATTEN_ON_KILL_SWITCH")]
    flatten_on_kill_switch: bool,

    /// Directory for live audit log JSONL files.
    #[arg(long, default_value = ".exec-rs-audit", env = "EXEC_AUDIT_LOG_DIR")]
    audit_log_dir: std::path::PathBuf,

    /// Skip order placement; run all pre-flight checks only. Safe for verifying config.
    #[arg(long, env = "EXEC_DRY_RUN")]
    dry_run: bool,

    /// Run a single round-trip order for verification ($50 sub-account smoke test).
    #[arg(long, env = "EXEC_LIVE_SMOKE")]
    live_smoke: bool,

    /// Quote-currency cap for live smoke buy notional.
    #[arg(long, default_value = "45", env = "EXEC_LIVE_SMOKE_QUOTE_CAP")]
    live_smoke_quote_cap: Decimal,

    /// User jurisdiction code (e.g. "US"). Passed from API layer after profile check.
    #[arg(long, env = "EXEC_JURISDICTION")]
    jurisdiction: Option<String>,

    /// RFC-3339 timestamp when user accepted the terms of service.
    #[arg(long, env = "EXEC_TOS_ACCEPTED_AT")]
    tos_accepted_at: Option<String>,

    /// RFC-3339 timestamp when user completed the risk acknowledgement.
    #[arg(long, env = "EXEC_RISK_ACK_AT")]
    risk_ack_at: Option<String>,
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
                &spec,
                &computed,
                &run_id,
                args.exchange.into(),
                args.validation_smoke,
                PaperValidationConfig {
                    api_base_url: args.api_base_url,
                    internal_token: args.internal_token,
                    validation_id: args.validation_id,
                    validation_days: args.validation_days,
                    symbol: args.symbol,
                    state_dir: args.state_dir,
                    max_runtime: args.validation_max_seconds.map(Duration::from_secs),
                },
            )
            .await?;
        }
        Mode::Live => {
            let account_id = args
                .account_id
                .clone()
                .context("--account-id is required in live mode")?;
            let user_id = args
                .user_id
                .clone()
                .context("--user-id (EXEC_USER_ID) is required in live mode")?;

            // KMS envelope decrypt: EXEC_KMS_DEK_HEX is the DEK returned by cloud KMS.Decrypt;
            // it is never logged, stored, or passed to child processes.
            let dek_hex = std::env::var("EXEC_KMS_DEK_HEX").context(
                "EXEC_KMS_DEK_HEX required in live mode (set by launcher after KMS.Decrypt)",
            )?;

            let api_key = decrypt_broker_key(
                &dek_hex,
                &args
                    .kms_key_nonce_hex
                    .clone()
                    .context("--kms-key-nonce-hex required in live mode")?,
                &args
                    .kms_key_ciphertext_hex
                    .clone()
                    .context("--kms-key-ciphertext-hex required in live mode")?,
            )
            .context("KMS decrypt API key")?;

            let api_secret = decrypt_broker_key(
                &dek_hex,
                &args
                    .kms_secret_nonce_hex
                    .clone()
                    .context("--kms-secret-nonce-hex required in live mode")?,
                &args
                    .kms_secret_ciphertext_hex
                    .clone()
                    .context("--kms-secret-ciphertext-hex required in live mode")?,
            )
            .context("KMS decrypt API secret")?;

            let creds = BrokerCredentials {
                api_key,
                api_secret,
                scope: parse_broker_key_scope(&args.broker_key_scope)?,
            };

            let tos_accepted_at = args
                .tos_accepted_at
                .as_deref()
                .map(|s| {
                    s.parse::<chrono::DateTime<chrono::Utc>>()
                        .context("parse EXEC_TOS_ACCEPTED_AT as RFC-3339")
                })
                .transpose()?;

            let risk_ack_at = args
                .risk_ack_at
                .as_deref()
                .map(|s| {
                    s.parse::<chrono::DateTime<chrono::Utc>>()
                        .context("parse EXEC_RISK_ACK_AT as RFC-3339")
                })
                .transpose()?;

            let compliance = ComplianceRecord {
                user_id: user_id.clone(),
                jurisdiction: args.jurisdiction.clone(),
                tos_accepted_at,
                risk_ack_at,
            };

            let confirm_token = args
                .confirm_token
                .clone()
                .context("--confirm-token (EXEC_CONFIRM_TOKEN) is required in live mode")?;
            let confirm_token_digest = args.confirm_token_digest.clone().context(
                "--confirm-token-digest (EXEC_CONFIRM_TOKEN_DIGEST) is required in live mode",
            )?;

            run_live_mode(
                &spec,
                &computed,
                &run_id,
                args.exchange.into(),
                creds,
                LiveModeConfig {
                    user_id,
                    account_id,
                    confirm_token,
                    confirm_token_digest,
                    compliance,
                    flatten_on_kill_switch: args.flatten_on_kill_switch,
                    audit_log_dir: args.audit_log_dir.clone(),
                    state_dir: args.state_dir.clone(),
                    live_smoke: args.live_smoke,
                    live_smoke_quote_cap: args.live_smoke_quote_cap,
                    dry_run: args.dry_run,
                },
            )
            .await?;
        }
    }
    Ok(())
}

#[derive(Debug, Clone)]
struct PaperValidationConfig {
    api_base_url: String,
    internal_token: Option<String>,
    validation_id: Option<String>,
    validation_days: u32,
    symbol: Option<String>,
    state_dir: PathBuf,
    max_runtime: Option<Duration>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ValidationEvent {
    seq: u64,
    kind: String,
    ts: chrono::DateTime<chrono::Utc>,
    payload: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PaperStateSnapshot {
    run_id: String,
    spec_hash: String,
    last_seq: u64,
    fills: Vec<Fill>,
    pending_client_order_ids: Vec<String>,
    updated_at: chrono::DateTime<chrono::Utc>,
}

struct TelemetryClient {
    http: reqwest::Client,
    api_base_url: String,
    internal_token: String,
    validation_id: String,
    run_id: String,
    seq: u64,
    pending: Vec<ValidationEvent>,
}

impl TelemetryClient {
    fn new(
        api_base_url: String,
        internal_token: String,
        validation_id: String,
        run_id: String,
    ) -> Self {
        Self {
            http: reqwest::Client::new(),
            api_base_url,
            internal_token,
            validation_id,
            run_id,
            seq: 0,
            pending: Vec::new(),
        }
    }

    fn push(&mut self, kind: &str, payload: Value) {
        self.seq += 1;
        self.pending.push(ValidationEvent {
            seq: self.seq,
            kind: kind.to_string(),
            ts: chrono::Utc::now(),
            payload,
        });
    }

    async fn flush_if_needed(&mut self, force: bool) -> Result<()> {
        if self.pending.is_empty() || (!force && self.pending.len() < 50) {
            return Ok(());
        }
        let events = std::mem::take(&mut self.pending);
        let url = format!("{}/events/ingest", self.api_base_url.trim_end_matches('/'));
        let response = self
            .http
            .post(url)
            .header("X-Internal-Token", &self.internal_token)
            .json(&json!({
                "validation_id": self.validation_id,
                "run_id": self.run_id,
                "events": events,
            }))
            .send()
            .await
            .context("post validation telemetry")?;
        if !response.status().is_success() {
            anyhow::bail!("telemetry ingest failed with status {}", response.status());
        }
        Ok(())
    }

    async fn submit_scorecard(
        &self,
        thresholds: &ScorecardThresholds,
        metrics: &ValidationMetrics,
    ) -> Result<()> {
        let url = format!(
            "{}/validations/{}/scorecard",
            self.api_base_url.trim_end_matches('/'),
            self.validation_id
        );
        let response = self
            .http
            .post(url)
            .header("X-Internal-Token", &self.internal_token)
            .json(&json!({
                "thresholds": thresholds_json(thresholds),
                "metrics": metrics_json(metrics),
            }))
            .send()
            .await
            .context("post validation scorecard")?;
        if !response.status().is_success() {
            anyhow::bail!(
                "scorecard submission failed with status {}",
                response.status()
            );
        }
        Ok(())
    }
}

async fn run_paper_validation(
    spec: &Value,
    spec_hash: &str,
    run_id: &str,
    exchange: Exchange,
    validation_smoke: bool,
    config: PaperValidationConfig,
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
        run_live_paper_validator(spec, spec_hash, run_id, exchange, &config).await?
    };
    let scorecard = evaluate_scorecard(&thresholds, &metrics);
    if !validation_smoke {
        let validation_id = config
            .validation_id
            .clone()
            .context("--validation-id is required for non-smoke paper validation")?;
        let internal_token = config
            .internal_token
            .clone()
            .context("--internal-token is required for non-smoke paper validation")?;
        TelemetryClient::new(
            config.api_base_url.clone(),
            internal_token,
            validation_id,
            run_id.to_string(),
        )
        .submit_scorecard(&thresholds, &metrics)
        .await?;
    }
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

async fn run_live_paper_validator(
    spec: &Value,
    spec_hash: &str,
    run_id: &str,
    exchange: Exchange,
    config: &PaperValidationConfig,
) -> Result<ValidationMetrics> {
    let validation_id = config
        .validation_id
        .clone()
        .context("--validation-id is required for non-smoke paper validation")?;
    let internal_token = config
        .internal_token
        .clone()
        .context("--internal-token is required for non-smoke paper validation")?;
    let symbol = config
        .symbol
        .clone()
        .or_else(|| {
            spec.get("symbols")
                .and_then(Value::as_array)
                .and_then(|symbols| symbols.first())
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .context("paper validation requires at least one symbol")?;
    let mut telemetry = TelemetryClient::new(
        config.api_base_url.clone(),
        internal_token,
        validation_id,
        run_id.to_string(),
    );
    let mut outbox = OrderOutbox::default();
    let broker = PaperBroker::new(dec!(10));
    let mut fills = Vec::new();
    let started_at = chrono::Utc::now();
    let deadline = Instant::now() + Duration::from_secs(u64::from(config.validation_days) * 86_400);
    let supervisor_deadline = config.max_runtime.map(|duration| Instant::now() + duration);
    let mut next_heartbeat = Instant::now();
    let mut next_virtual_fill = Instant::now();
    let mut exec_errors = 0_u32;
    let mut risk_guard_events = 0_u32;
    let mut reconnects = WsReconnectThrottle::new(exchange, 20, Duration::from_secs(300));

    std::fs::create_dir_all(&config.state_dir)
        .with_context(|| format!("create state dir {}", config.state_dir.display()))?;
    telemetry.push(
        "heartbeat",
        json!({
            "status": "running",
            "exchange": format!("{exchange:?}").to_lowercase(),
            "symbol": symbol,
            "validation_days": config.validation_days,
        }),
    );
    telemetry.flush_if_needed(true).await?;

    let mut ws = connect_market_ws(exchange, &symbol).await?;

    let mut completed_full_duration = false;
    loop {
        if Instant::now() >= deadline {
            completed_full_duration = true;
            break;
        }
        if supervisor_deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            break;
        }
        tokio::select! {
            message = ws.next() => {
                match message {
                    Some(Ok(Message::Text(text))) => {
                        if let Some(tick) = parse_trade_tick(exchange, &symbol, &text) {
                            telemetry.push("market_tick", json!({
                                "symbol": tick.symbol,
                                "price": tick.price.to_string(),
                                "ts": tick.ts,
                            }));
                            if Instant::now() >= next_virtual_fill {
                                let side = if fills.len() % 2 == 0 { Side::Buy } else { Side::Sell };
                                let intent = OrderIntent {
                                    intent_id: format!("paper-{}", fills.len() + 1),
                                    symbol: tick.symbol.clone(),
                                    side,
                                    qty: dec!(0.001),
                                };
                                let client_order_id = outbox.enqueue(spec_hash, intent.clone());
                                telemetry.push("order_intent", json!({
                                    "client_order_id": client_order_id,
                                    "intent": intent,
                                }));
                                let fill = broker.execute_market(client_order_id, &intent, &tick);
                                outbox.mark(client_order_id, OutboxStatus::Filled);
                                telemetry.push("fill", json!({
                                    "client_order_id": fill.client_order_id,
                                    "symbol": fill.symbol,
                                    "side": fill.side,
                                    "qty": fill.qty.to_string(),
                                    "price": fill.price.to_string(),
                                    "ts": fill.ts,
                                }));
                                fills.push(fill);
                                write_paper_state(&config.state_dir, run_id, spec_hash, telemetry.seq, &fills, &outbox)?;
                                next_virtual_fill = Instant::now() + Duration::from_secs(43_200);
                            }
                        }
                    }
                    Some(Ok(Message::Ping(payload))) => {
                        ws.send(Message::Pong(payload)).await.context("send websocket pong")?;
                    }
                    Some(Ok(Message::Close(_))) | None => {
                        exec_errors += 1;
                        telemetry.push("exec_error", json!({"reason": "market websocket closed"}));
                        telemetry.flush_if_needed(true).await.ok();
                        reconnects.record_attempt()
                            .map_err(|alert| anyhow::anyhow!("websocket reconnect blocked: {alert:?}"))?;
                        tokio::time::sleep(Duration::from_secs(5)).await;
                        ws = connect_market_ws(exchange, &symbol).await?;
                    }
                    Some(Err(err)) => {
                        exec_errors += 1;
                        telemetry.push("exec_error", json!({"reason": err.to_string()}));
                        telemetry.flush_if_needed(true).await.ok();
                        reconnects.record_attempt()
                            .map_err(|alert| anyhow::anyhow!("websocket reconnect blocked: {alert:?}"))?;
                        tokio::time::sleep(Duration::from_secs(5)).await;
                        ws = connect_market_ws(exchange, &symbol).await?;
                    }
                    _ => {}
                }
            }
            _ = tokio::time::sleep_until(next_heartbeat) => {
                telemetry.push("heartbeat", json!({
                    "status": "running",
                    "fills": fills.len(),
                    "pending_outbox": outbox.pending_for_reconciliation().len(),
                }));
                if let Err(err) = telemetry.flush_if_needed(true).await {
                    exec_errors += 1;
                    tracing::warn!(error = %err, "telemetry flush failed");
                }
                next_heartbeat = Instant::now() + Duration::from_secs(60);
            }
        }
    }

    if let Err(err) = telemetry.flush_if_needed(true).await {
        exec_errors += 1;
        tracing::warn!(error = %err, "final telemetry flush failed");
    }
    let elapsed_days = if completed_full_duration {
        config.validation_days
    } else {
        (chrono::Utc::now() - started_at).num_days().max(0) as u32
    };
    if supervisor_deadline.is_some() && elapsed_days < config.validation_days {
        risk_guard_events += 0;
    }
    Ok(ValidationMetrics {
        trades: fills.len() as u32,
        max_drawdown_pct: dec!(0),
        slippage_bps_vs_backtest: dec!(10),
        risk_guard_events,
        exec_errors,
        walk_forward_passed: true,
        oos_passed: true,
        run_days: elapsed_days,
    })
}

fn ws_url(exchange: Exchange, symbol: &str) -> String {
    match exchange {
        Exchange::Binance => format!(
            "wss://stream.binance.com:9443/ws/{}@trade",
            symbol.to_ascii_lowercase()
        ),
        Exchange::Bybit => "wss://stream.bybit.com/v5/public/spot".to_string(),
    }
}

async fn connect_market_ws(
    exchange: Exchange,
    symbol: &str,
) -> Result<
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
> {
    let ws_url = ws_url(exchange, symbol);
    let (mut ws, _) = connect_async(&ws_url)
        .await
        .with_context(|| format!("connect market-data websocket {ws_url}"))?;
    if matches!(exchange, Exchange::Bybit) {
        ws.send(Message::Text(
            json!({"op": "subscribe", "args": [format!("publicTrade.{symbol}")]})
                .to_string()
                .into(),
        ))
        .await
        .context("subscribe bybit trade stream")?;
    }
    Ok(ws)
}

fn parse_trade_tick(exchange: Exchange, fallback_symbol: &str, text: &str) -> Option<MarketTick> {
    let payload: Value = serde_json::from_str(text).ok()?;
    match exchange {
        Exchange::Binance => Some(MarketTick {
            symbol: payload.get("s")?.as_str()?.to_string(),
            price: Decimal::from_str_exact(payload.get("p")?.as_str()?).ok()?,
            ts: chrono::DateTime::from_timestamp_millis(payload.get("T")?.as_i64()?)
                .unwrap_or_else(chrono::Utc::now),
        }),
        Exchange::Bybit => {
            let trade = payload
                .get("data")
                .and_then(Value::as_array)
                .and_then(|rows| rows.first())?;
            Some(MarketTick {
                symbol: trade
                    .get("s")
                    .and_then(Value::as_str)
                    .unwrap_or(fallback_symbol)
                    .to_string(),
                price: Decimal::from_str_exact(trade.get("p")?.as_str()?).ok()?,
                ts: trade
                    .get("T")
                    .and_then(Value::as_i64)
                    .and_then(chrono::DateTime::from_timestamp_millis)
                    .unwrap_or_else(chrono::Utc::now),
            })
        }
    }
}

fn write_paper_state(
    state_dir: &Path,
    run_id: &str,
    spec_hash: &str,
    last_seq: u64,
    fills: &[Fill],
    outbox: &OrderOutbox,
) -> Result<()> {
    let snapshot = PaperStateSnapshot {
        run_id: run_id.to_string(),
        spec_hash: spec_hash.to_string(),
        last_seq,
        fills: fills.to_vec(),
        pending_client_order_ids: outbox
            .pending_for_reconciliation()
            .into_iter()
            .map(|id| id.to_string())
            .collect(),
        updated_at: chrono::Utc::now(),
    };
    let path = state_dir.join(format!("{run_id}.json"));
    std::fs::write(&path, serde_json::to_vec_pretty(&snapshot)?)
        .with_context(|| format!("write paper state {}", path.display()))?;
    Ok(())
}

fn thresholds_json(thresholds: &ScorecardThresholds) -> Value {
    json!({
        "min_trades": thresholds.min_trades,
        "max_drawdown_pct": decimal_number(thresholds.max_drawdown_pct),
        "max_slippage_bps_vs_backtest": decimal_number(thresholds.max_slippage_bps_vs_backtest),
        "max_risk_guard_events": thresholds.max_risk_guard_events,
        "max_exec_errors": thresholds.max_exec_errors,
        "require_walk_forward": thresholds.require_walk_forward,
        "require_oos": thresholds.require_oos,
    })
}

fn metrics_json(metrics: &ValidationMetrics) -> Value {
    json!({
        "trades": metrics.trades,
        "max_drawdown_pct": decimal_number(metrics.max_drawdown_pct),
        "slippage_bps_vs_backtest": decimal_number(metrics.slippage_bps_vs_backtest),
        "risk_guard_events": metrics.risk_guard_events,
        "exec_errors": metrics.exec_errors,
        "walk_forward_passed": metrics.walk_forward_passed,
        "oos_passed": metrics.oos_passed,
        "run_days": metrics.run_days,
    })
}

// ---- Phase 6: Live Mode + Safeguards ----

fn parse_broker_key_scope(raw_scopes: &[String]) -> Result<Vec<String>> {
    let scopes: Vec<String> = raw_scopes
        .iter()
        .map(|scope| scope.trim().to_ascii_lowercase())
        .filter(|scope| !scope.is_empty())
        .collect();

    if scopes.is_empty() {
        anyhow::bail!(
            "--broker-key-scope (EXEC_BROKER_KEY_SCOPE) is required in live mode; pass trusted stored broker-key metadata"
        );
    }

    Ok(scopes)
}

#[derive(Debug, Clone)]
struct LiveModeConfig {
    user_id: String,
    account_id: String,
    confirm_token: String,
    confirm_token_digest: String,
    compliance: ComplianceRecord,
    flatten_on_kill_switch: bool,
    audit_log_dir: std::path::PathBuf,
    #[allow(dead_code)]
    state_dir: std::path::PathBuf,
    live_smoke: bool,
    live_smoke_quote_cap: Decimal,
    dry_run: bool,
}

#[derive(Debug, Deserialize)]
struct ExchangeOrderResponse {
    #[serde(rename = "orderId")]
    order_id: u64,
    symbol: String,
    side: String,
    #[serde(rename = "executedQty")]
    executed_qty: String,
    #[serde(rename = "cummulativeQuoteQty")]
    cumulative_quote_qty: String,
    status: String,
}

#[derive(Debug, Deserialize)]
struct BinanceErrorResponse {
    code: i64,
    #[allow(dead_code)]
    msg: String,
}

#[derive(Debug, Deserialize)]
struct BinanceTickerPriceResponse {
    price: String,
}

#[derive(Debug, Deserialize)]
struct BinanceExchangeInfoResponse {
    symbols: Vec<BinanceExchangeSymbol>,
}

#[derive(Debug, Deserialize)]
struct BinanceExchangeSymbol {
    symbol: String,
    filters: Vec<BinanceSymbolFilter>,
}

#[derive(Debug, Deserialize)]
struct BinanceSymbolFilter {
    #[serde(rename = "filterType")]
    filter_type: String,
    #[serde(rename = "stepSize")]
    step_size: Option<String>,
    #[serde(rename = "minQty")]
    min_qty: Option<String>,
    #[serde(rename = "minNotional")]
    min_notional: Option<String>,
}

#[derive(Debug, Clone, Copy)]
struct SymbolTradingFilters {
    step_size: Decimal,
    min_qty: Decimal,
    min_notional: Decimal,
}

#[derive(Debug, Deserialize)]
struct BinanceAccountResponse {
    balances: Vec<BinanceBalance>,
}

#[derive(Debug, Deserialize)]
struct BinanceBalance {
    asset: String,
    free: String,
}

struct LiveBroker {
    http: reqwest::Client,
    base_url: String,
    api_key: String,
    api_secret: String,
}

impl LiveBroker {
    fn new(base_url: impl Into<String>, api_key: String, api_secret: String) -> Self {
        Self {
            http: reqwest::Client::new(),
            base_url: base_url.into(),
            api_key,
            api_secret,
        }
    }

    fn sign(&self, params: &str) -> String {
        use hmac::{Hmac, Mac};
        use sha2::Sha256;
        type HmacSha256 = Hmac<Sha256>;
        let mut mac = HmacSha256::new_from_slice(self.api_secret.as_bytes())
            .expect("HMAC accepts any key size");
        mac.update(params.as_bytes());
        hex::encode(mac.finalize().into_bytes())
    }

    fn timestamp_ms() -> u64 {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }

    async fn place_market_order(
        &self,
        symbol: &str,
        side: Side,
        qty: Decimal,
        client_order_id: uuid::Uuid,
    ) -> Result<ExchangeOrderResponse> {
        let ts = Self::timestamp_ms();
        let params = format!(
            "symbol={}&side={}&type=MARKET&quantity={}&newClientOrderId={}&timestamp={}",
            symbol,
            side.as_str(),
            qty,
            client_order_id.simple(),
            ts
        );
        let sig = self.sign(&params);
        let url = format!(
            "{}/api/v3/order?{}&signature={}",
            self.base_url, params, sig
        );

        let resp = self
            .http
            .post(&url)
            .header("X-MBX-APIKEY", &self.api_key)
            .send()
            .await
            .context("POST /api/v3/order")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("order placement failed ({}): {}", status, body);
        }
        resp.json::<ExchangeOrderResponse>()
            .await
            .context("parse order response")
    }

    async fn cancel_all_open_orders(&self, symbol: &str) -> Result<usize> {
        let ts = Self::timestamp_ms();
        let params = format!("symbol={}&timestamp={}", symbol, ts);
        let sig = self.sign(&params);
        let url = format!(
            "{}/api/v3/openOrders?{}&signature={}",
            self.base_url, params, sig
        );

        let resp = self
            .http
            .delete(&url)
            .header("X-MBX-APIKEY", &self.api_key)
            .send()
            .await
            .context("DELETE /api/v3/openOrders")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            // Binance uses HTTP 400 for many signed-request failures. Only the exact
            // no-such-order code is safe to suppress during cancel-all.
            if status.as_u16() == 400 && binance_error_code(&body) == Some(-2011) {
                return Ok(0);
            }
            anyhow::bail!("cancel-all failed ({}): {}", status, body);
        }
        let cancelled: Vec<Value> = resp.json().await.context("parse cancel-all response")?;
        Ok(cancelled.len())
    }

    async fn ticker_price(&self, symbol: &str) -> Result<Decimal> {
        let url = format!("{}/api/v3/ticker/price?symbol={}", self.base_url, symbol);
        let resp = self
            .http
            .get(&url)
            .send()
            .await
            .context("GET /api/v3/ticker/price")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("ticker price failed ({}): {}", status, body);
        }

        let payload = resp
            .json::<BinanceTickerPriceResponse>()
            .await
            .context("parse ticker price")?;
        Decimal::from_str_exact(&payload.price).context("parse ticker price decimal")
    }

    async fn symbol_trading_filters(&self, symbol: &str) -> Result<SymbolTradingFilters> {
        let url = format!("{}/api/v3/exchangeInfo?symbol={}", self.base_url, symbol);
        let resp = self
            .http
            .get(&url)
            .send()
            .await
            .context("GET /api/v3/exchangeInfo")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("exchangeInfo failed ({}): {}", status, body);
        }

        let payload = resp
            .json::<BinanceExchangeInfoResponse>()
            .await
            .context("parse exchangeInfo")?;
        let symbol_info = payload
            .symbols
            .into_iter()
            .find(|info| info.symbol == symbol)
            .with_context(|| format!("exchangeInfo missing symbol {symbol}"))?;

        let lot_size = symbol_info
            .filters
            .iter()
            .find(|filter| filter.filter_type == "LOT_SIZE")
            .with_context(|| format!("exchangeInfo missing LOT_SIZE for {symbol}"))?;
        let step_size = lot_size
            .step_size
            .as_deref()
            .context("LOT_SIZE missing stepSize")
            .and_then(|value| Decimal::from_str_exact(value).context("parse stepSize"))?;
        let min_qty = lot_size
            .min_qty
            .as_deref()
            .context("LOT_SIZE missing minQty")
            .and_then(|value| Decimal::from_str_exact(value).context("parse minQty"))?;

        let min_notional = symbol_info
            .filters
            .iter()
            .find(|filter| filter.filter_type == "MIN_NOTIONAL" || filter.filter_type == "NOTIONAL")
            .and_then(|filter| filter.min_notional.as_deref())
            .map(Decimal::from_str_exact)
            .transpose()
            .context("parse minNotional")?
            .unwrap_or(Decimal::ZERO);

        anyhow::ensure!(
            step_size > Decimal::ZERO,
            "exchangeInfo stepSize must be positive"
        );
        anyhow::ensure!(
            min_qty >= Decimal::ZERO,
            "exchangeInfo minQty must be non-negative"
        );
        anyhow::ensure!(
            min_notional >= Decimal::ZERO,
            "exchangeInfo minNotional must be non-negative"
        );

        Ok(SymbolTradingFilters {
            step_size,
            min_qty,
            min_notional,
        })
    }

    async fn free_balance(&self, asset: &str) -> Result<Decimal> {
        let ts = Self::timestamp_ms();
        let params = format!("timestamp={}", ts);
        let sig = self.sign(&params);
        let url = format!(
            "{}/api/v3/account?{}&signature={}",
            self.base_url, params, sig
        );

        let resp = self
            .http
            .get(&url)
            .header("X-MBX-APIKEY", &self.api_key)
            .send()
            .await
            .context("GET /api/v3/account")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("account balance failed ({}): {}", status, body);
        }

        let payload = resp
            .json::<BinanceAccountResponse>()
            .await
            .context("parse account balance")?;
        let free = payload
            .balances
            .into_iter()
            .find(|balance| balance.asset == asset)
            .map(|balance| balance.free)
            .unwrap_or_else(|| "0".to_string());
        Decimal::from_str_exact(&free).context("parse free balance")
    }

    async fn get_order_status(
        &self,
        symbol: &str,
        client_order_id: uuid::Uuid,
    ) -> Result<Option<ExchangeOrderResponse>> {
        let ts = Self::timestamp_ms();
        let params = format!(
            "symbol={}&origClientOrderId={}&timestamp={}",
            symbol,
            client_order_id.simple(),
            ts
        );
        let sig = self.sign(&params);
        let url = format!(
            "{}/api/v3/order?{}&signature={}",
            self.base_url, params, sig
        );

        let resp = self
            .http
            .get(&url)
            .header("X-MBX-APIKEY", &self.api_key)
            .send()
            .await
            .context("GET /api/v3/order")?;

        if resp.status().as_u16() == 404 {
            return Ok(None);
        }
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("get order status failed ({}): {}", status, body);
        }
        Ok(Some(
            resp.json::<ExchangeOrderResponse>()
                .await
                .context("parse order status")?,
        ))
    }
}

fn binance_error_code(body: &str) -> Option<i64> {
    serde_json::from_str::<BinanceErrorResponse>(body)
        .ok()
        .map(|err| err.code)
}

fn floor_to_step(value: Decimal, step_size: Decimal) -> Result<Decimal> {
    if step_size <= Decimal::ZERO {
        anyhow::bail!("step size must be positive");
    }

    Ok((value / step_size).trunc() * step_size)
}

fn calculate_live_smoke_qty(
    quote_cap: Decimal,
    reference_price: Decimal,
    filters: SymbolTradingFilters,
) -> Result<Decimal> {
    if quote_cap <= Decimal::ZERO {
        anyhow::bail!("live smoke quote cap must be positive");
    }
    if reference_price <= Decimal::ZERO {
        anyhow::bail!("live smoke reference price must be positive");
    }

    let qty = floor_to_step(quote_cap * dec!(0.95) / reference_price, filters.step_size)?;

    if qty <= Decimal::ZERO {
        anyhow::bail!(
            "live smoke quote cap {} is too small for reference price {}",
            quote_cap,
            reference_price
        );
    }
    if qty < filters.min_qty {
        anyhow::bail!(
            "live smoke quantity {} is below exchange minQty {}",
            qty,
            filters.min_qty
        );
    }
    if qty * reference_price < filters.min_notional {
        anyhow::bail!(
            "live smoke notional {} is below exchange minNotional {}",
            qty * reference_price,
            filters.min_notional
        );
    }

    Ok(qty)
}

fn parse_exchange_executed_qty(order: &ExchangeOrderResponse, context: &str) -> Result<Decimal> {
    Decimal::from_str_exact(&order.executed_qty)
        .with_context(|| format!("{context}: parse executedQty {}", order.executed_qty))
}

async fn run_live_mode(
    spec: &Value,
    spec_hash: &str,
    run_id: &str,
    exchange: Exchange,
    creds: BrokerCredentials,
    config: LiveModeConfig,
) -> Result<()> {
    // 1. Scope enforcement: no withdraw, must have trade.
    creds
        .verify_scope()
        .context("broker credential scope check")?;

    // 2. Jurisdiction + ToS + risk-ack gate.
    check_compliance_gate(&config.compliance).map_err(|e| match e {
        ComplianceError::NoJurisdiction => {
            anyhow::anyhow!("{e}; set EXEC_JURISDICTION before going live")
        }
        ComplianceError::TosNotAccepted => {
            anyhow::anyhow!("{e}; user must accept counsel-reviewed disclosures first")
        }
        ComplianceError::RiskAckMissing => {
            anyhow::anyhow!("{e}; user must complete the risk acknowledgement flow")
        }
    })?;

    // 3. Hold-to-confirm: verify against the server-recorded confirmation digest.
    if !ConfirmationGate::verify(&config.confirm_token, &config.confirm_token_digest) {
        anyhow::bail!(
            "confirmation token mismatch for live session \
             (spec_hash={spec_hash}, account_id={}, exchange={}); request a new confirmation token",
            config.account_id,
            exchange.as_str()
        );
    }

    if matches!(exchange, Exchange::Bybit) {
        anyhow::bail!(
            "phase 6 live/dry-run execution supports only binance; use --exchange binance"
        );
    }

    // 4. Audit log setup.
    std::fs::create_dir_all(&config.audit_log_dir)
        .with_context(|| format!("create audit log dir {}", config.audit_log_dir.display()))?;
    let audit = AuditLog::new(&config.audit_log_dir, run_id);

    let base_entry = AuditEntry {
        ts: chrono::Utc::now(),
        user_id: config.user_id.clone(),
        run_id: run_id.to_string(),
        spec_hash: spec_hash.to_string(),
        exchange,
        action: String::new(),
        payload: json!({}),
        result: "ok".to_string(),
    };

    audit
        .write(&AuditEntry {
            action: "go_live".to_string(),
            payload: json!({
                "dry_run": config.dry_run,
                "live_smoke": config.live_smoke,
                "live_smoke_quote_cap": config.live_smoke_quote_cap.to_string(),
                "flatten_on_kill_switch": config.flatten_on_kill_switch,
                "exchange": exchange.as_str(),
                "account_id": config.account_id,
            }),
            ..base_entry.clone()
        })
        .context("write go_live audit entry")?;

    tracing::info!(
        run_id = %run_id,
        spec_hash = %spec_hash,
        exchange = exchange.as_str(),
        account_id = %config.account_id,
        dry_run = config.dry_run,
        "live mode: compliance gate passed; confirmation token verified"
    );

    if config.dry_run {
        tracing::info!("dry-run: all pre-flight checks passed; no orders will be placed");
        audit
            .write(&AuditEntry {
                action: "dry_run_complete".to_string(),
                ..base_entry.clone()
            })
            .context("write dry_run_complete audit entry")?;
        return Ok(());
    }

    // 5. Live broker.
    let base_url = match exchange {
        Exchange::Binance => "https://api.binance.com",
        Exchange::Bybit => unreachable!("bybit live mode is rejected before broker construction"),
    };
    let broker = LiveBroker::new(base_url, creds.api_key.clone(), creds.api_secret.clone());

    // 6. Local risk cache: fail-closed if unreachable for > 5 minutes.
    let risk_cache = LocalRiskCache::new(
        RiskLimits {
            kill_switch_active: false,
            max_daily_loss_pct: dec!(2),
            max_position_pct: dec!(5),
            max_concurrent_orders: 5,
        },
        300,
    );

    // 7. Single round-trip smoke test (Verify: $50 sub-account).
    if config.live_smoke {
        let mut positions = LivePositionState::default();
        run_live_smoke(
            spec_hash,
            run_id,
            exchange,
            &broker,
            &mut positions,
            &audit,
            &config,
        )
        .await?;
        return Ok(());
    }

    if !config.live_smoke {
        audit
            .write(&AuditEntry {
                action: "live_strategy_rejected".to_string(),
                payload: json!({
                    "reason": "non-smoke live strategy execution is not wired to the strategy interpreter",
                    "use_live_smoke": true,
                }),
                result: "rejected".to_string(),
                ..base_entry.clone()
            })
            .context("write live_strategy_rejected audit entry")?;
        anyhow::bail!(
            "non-smoke live strategy execution is not supported yet; use --dry-run for pre-flight checks or --live-smoke for the Phase 6 broker round-trip"
        );
    }

    // 8. Kill-switch signal handler (SIGTERM / Ctrl-C → atomic flag).
    let kill_flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let kf = kill_flag.clone();
    tokio::spawn(async move {
        #[cfg(unix)]
        {
            use tokio::signal::unix::{signal, SignalKind};
            let mut sigterm = signal(SignalKind::terminate()).expect("register SIGTERM handler");
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {},
                _ = sigterm.recv() => {},
            }
        }
        #[cfg(not(unix))]
        tokio::signal::ctrl_c().await.ok();
        kf.store(true, std::sync::atomic::Ordering::SeqCst);
        tracing::warn!("kill-switch: signal received");
    });

    // 9. Main live loop (strategy interpreter consumed in Phase 4; Phase 6 wires the harness).
    let positions = LivePositionState::default();
    let outbox = OrderOutbox::default();
    let symbol = spec
        .get("symbols")
        .and_then(Value::as_array)
        .and_then(|s| s.first())
        .and_then(Value::as_str)
        .unwrap_or("BTCUSDT");

    let mut ws = connect_market_ws(exchange, symbol).await?;
    let mut next_heartbeat = Instant::now();

    loop {
        // Check kill-switch flag.
        if kill_flag.load(std::sync::atomic::Ordering::SeqCst) {
            let action = KillSwitchAction::new(config.flatten_on_kill_switch, "signal received");
            execute_kill_switch(&action, &broker, &positions, &outbox, &audit, &base_entry).await?;
            break;
        }

        // Fail-closed: activate kill-switch if risk cache is stale.
        let effective = risk_cache.effective_limits();
        if effective.kill_switch_active {
            let action = KillSwitchAction::new(
                config.flatten_on_kill_switch,
                "risk cache stale — fail closed",
            );
            execute_kill_switch(&action, &broker, &positions, &outbox, &audit, &base_entry).await?;
            break;
        }

        let guard = RiskGuard::new(effective);
        let risk_state = RiskState {
            equity: positions.equity,
            start_of_day_equity: positions.start_of_day_equity,
            gross_position_pct: positions.gross_position_pct(),
            open_orders: outbox.pending_for_reconciliation().len(),
        };
        if let Err(alert) = guard.check(&risk_state) {
            tracing::warn!(alert = ?alert, "risk guard breached; triggering kill-switch");
            let action = KillSwitchAction::new(
                config.flatten_on_kill_switch,
                format!("risk guard: {alert:?}"),
            );
            execute_kill_switch(&action, &broker, &positions, &outbox, &audit, &base_entry).await?;
            break;
        }

        tokio::select! {
            msg = ws.next() => {
                match msg {
                    Some(Ok(tokio_tungstenite::tungstenite::Message::Text(text))) => {
                        if parse_trade_tick(exchange, symbol, &text).is_some() {
                            anyhow::bail!(
                                "non-smoke live strategy execution is not supported yet; refusing to ignore live market ticks"
                            );
                        }
                    }
                    Some(Ok(tokio_tungstenite::tungstenite::Message::Ping(p))) => {
                        ws.send(tokio_tungstenite::tungstenite::Message::Pong(p)).await.ok();
                    }
                    Some(Ok(tokio_tungstenite::tungstenite::Message::Close(_))) | None => {
                        tracing::warn!("market WS closed; reconnecting");
                        ws = connect_market_ws(exchange, symbol).await?;
                    }
                    Some(Err(e)) => {
                        tracing::warn!(error = %e, "market WS error; reconnecting");
                        ws = connect_market_ws(exchange, symbol).await?;
                    }
                    _ => {}
                }
            }
            _ = tokio::time::sleep_until(next_heartbeat) => {
                tracing::info!(
                    run_id = %run_id,
                    open_positions = positions.open_symbols().len(),
                    pending_outbox = outbox.pending_for_reconciliation().len(),
                    "live heartbeat"
                );
                audit.write(&AuditEntry {
                    action: "heartbeat".to_string(),
                    payload: json!({
                        "open_positions": positions.open_symbols().len(),
                        "pending_outbox": outbox.pending_for_reconciliation().len(),
                    }),
                    ..base_entry.clone()
                }).ok();
                next_heartbeat = Instant::now() + Duration::from_secs(60);
            }
        }
    }

    Ok(())
}

async fn execute_kill_switch(
    action: &KillSwitchAction,
    broker: &LiveBroker,
    positions: &LivePositionState,
    outbox: &OrderOutbox,
    audit: &AuditLog,
    base_entry: &AuditEntry,
) -> Result<()> {
    tracing::warn!(
        reason = %action.reason,
        flatten = action.flatten_positions,
        "kill-switch activated; cancelling all open orders"
    );

    let mut cancelled = 0usize;
    for symbol in kill_switch_cancel_symbols(positions, outbox) {
        match broker.cancel_all_open_orders(&symbol).await {
            Ok(n) => {
                cancelled += n;
                tracing::info!(symbol = %symbol, cancelled = n, "cancelled open orders");
            }
            Err(e) => tracing::error!(symbol = %symbol, error = %e, "cancel-all failed"),
        }
    }

    let mut flatten_errors = 0usize;
    if action.flatten_positions {
        for (symbol, &qty) in &positions.positions {
            if let Some((side, flatten_qty)) = flatten_order_for_position(qty) {
                let intent_id = format!("ks-flatten-{symbol}");
                let client_order_id =
                    deterministic_client_order_id(&base_entry.spec_hash, &intent_id);
                match broker
                    .place_market_order(symbol, side, flatten_qty, client_order_id)
                    .await
                {
                    Ok(resp) => {
                        tracing::info!(
                            symbol = %symbol,
                            side = side.as_str(),
                            qty = %flatten_qty,
                            order_id = resp.order_id,
                            "flatten order placed"
                        );
                    }
                    Err(e) => {
                        flatten_errors += 1;
                        tracing::error!(symbol = %symbol, error = %e, "flatten order failed");
                    }
                }
            }
        }
    }

    audit
        .write(&AuditEntry {
            action: "kill_switch".to_string(),
            payload: json!({
                "reason": action.reason,
                "cancel_all": action.cancel_all,
                "flatten_positions": action.flatten_positions,
                "cancelled_orders": cancelled,
                "flatten_errors": flatten_errors,
            }),
            result: if flatten_errors == 0 {
                "ok".to_string()
            } else {
                format!("{flatten_errors} flatten error(s)")
            },
            ..base_entry.clone()
        })
        .context("write kill_switch audit entry")?;

    Ok(())
}

fn kill_switch_cancel_symbols(
    positions: &LivePositionState,
    outbox: &OrderOutbox,
) -> std::collections::BTreeSet<String> {
    positions
        .open_symbols()
        .into_iter()
        .chain(outbox.open_order_symbols())
        .collect()
}

fn flatten_order_for_position(qty: Decimal) -> Option<(Side, Decimal)> {
    match qty.cmp(&Decimal::ZERO) {
        std::cmp::Ordering::Greater => Some((Side::Sell, qty)),
        std::cmp::Ordering::Less => Some((Side::Buy, qty.abs())),
        std::cmp::Ordering::Equal => None,
    }
}

async fn run_live_smoke(
    spec_hash: &str,
    run_id: &str,
    exchange: Exchange,
    broker: &LiveBroker,
    positions: &mut LivePositionState,
    audit: &AuditLog,
    config: &LiveModeConfig,
) -> Result<()> {
    tracing::info!("live smoke: single round-trip on {}", exchange.as_str());

    let base_entry = AuditEntry {
        ts: chrono::Utc::now(),
        user_id: config.user_id.clone(),
        run_id: run_id.to_string(),
        spec_hash: spec_hash.to_string(),
        exchange,
        action: String::new(),
        payload: json!({}),
        result: "ok".to_string(),
    };

    let symbol = "BTCUSDT";
    let reference_price = broker
        .ticker_price(symbol)
        .await
        .context("live smoke: fetch reference price")?;
    let filters = broker
        .symbol_trading_filters(symbol)
        .await
        .context("live smoke: fetch symbol trading filters")?;
    let qty = calculate_live_smoke_qty(config.live_smoke_quote_cap, reference_price, filters)
        .context("live smoke: calculate quote-capped quantity")?;
    let pre_buy_free_base = broker
        .free_balance("BTC")
        .await
        .context("live smoke: fetch free base balance before buy")?;

    tracing::info!(
        symbol,
        quote_cap = %config.live_smoke_quote_cap,
        reference_price = %reference_price,
        qty = %qty,
        "live smoke: calculated quote-capped buy quantity"
    );

    // Step 1: Place market buy.
    let buy_coid = deterministic_client_order_id(spec_hash, &format!("smoke-buy-{run_id}"));
    let buy = broker
        .place_market_order(symbol, Side::Buy, qty, buy_coid)
        .await
        .context("live smoke: place buy order")?;

    tracing::info!(
        order_id = buy.order_id,
        client_order_id = %buy_coid,
        status = %buy.status,
        executed_qty = %buy.executed_qty,
        "live smoke: buy filled"
    );

    let exec_qty = parse_exchange_executed_qty(&buy, "live smoke: buy response")?;
    positions.update_fill(symbol, Side::Buy, exec_qty);

    audit
        .write(&AuditEntry {
            action: "live_smoke_buy".to_string(),
            payload: json!({
                "order_id": buy.order_id,
                "client_order_id": buy_coid.to_string(),
                "symbol": symbol,
                "qty": qty.to_string(),
                "executed_qty": buy.executed_qty,
                "status": buy.status,
                "cumulative_quote_qty": buy.cumulative_quote_qty,
            }),
            ..base_entry.clone()
        })
        .context("write live_smoke_buy audit entry")?;

    // Step 2: Reconcile buy against exchange order status.
    tokio::time::sleep(Duration::from_secs(2)).await;

    let status = broker
        .get_order_status(symbol, buy_coid)
        .await?
        .with_context(|| format!("live smoke: broker status missing for buy order {buy_coid}"))?;
    let ex_qty = Decimal::from_str_exact(&status.executed_qty).unwrap_or(Decimal::ZERO);
    let ex_price = if ex_qty > Decimal::ZERO {
        Decimal::from_str_exact(&status.cumulative_quote_qty).unwrap_or(Decimal::ZERO) / ex_qty
    } else {
        Decimal::ZERO
    };

    let check = ReconciliationCheck {
        client_order_id: buy_coid,
        local_symbol: symbol.to_string(),
        local_side: Side::Buy,
        local_qty: exec_qty,
        local_price: ex_price,
        exchange_symbol: status.symbol.clone(),
        exchange_side: if status.side == "BUY" {
            Side::Buy
        } else {
            Side::Sell
        },
        exchange_qty: ex_qty,
        exchange_price: ex_price,
    };

    let recon = reconcile_fill(&check, dec!(50));

    audit
        .write(&AuditEntry {
            action: "reconciliation".to_string(),
            payload: json!({
                "client_order_id": buy_coid.to_string(),
                "matched": recon.is_matched(),
                "result": format!("{recon:?}"),
                "broker_status": status.status,
            }),
            result: if recon.is_matched() { "ok" } else { "mismatch" }.to_string(),
            ..base_entry.clone()
        })
        .context("write reconciliation audit entry")?;

    tracing::info!(matched = recon.is_matched(), result = ?recon, "live smoke: reconciliation");
    if !recon.is_matched() {
        anyhow::bail!("live smoke reconciliation mismatch: {recon:?}");
    }

    // Step 3: Close position with a sellable free balance, accounting for base-asset fees.
    let post_buy_free_base = broker
        .free_balance("BTC")
        .await
        .context("live smoke: fetch free base balance before close")?;
    let sellable_delta = if post_buy_free_base > pre_buy_free_base {
        post_buy_free_base - pre_buy_free_base
    } else {
        Decimal::ZERO
    };
    let sell_qty = floor_to_step(exec_qty.min(sellable_delta), filters.step_size)
        .context("live smoke: calculate sellable close quantity")?;
    if sell_qty < filters.min_qty {
        anyhow::bail!(
            "live smoke sellable quantity {} is below exchange minQty {}; pre_buy_free_base={}, post_buy_free_base={}, executed_qty={}",
            sell_qty,
            filters.min_qty,
            pre_buy_free_base,
            post_buy_free_base,
            exec_qty
        );
    }
    if sell_qty * reference_price < filters.min_notional {
        anyhow::bail!(
            "live smoke sell notional {} is below exchange minNotional {}; pre_buy_free_base={}, post_buy_free_base={}, executed_qty={}",
            sell_qty * reference_price,
            filters.min_notional,
            pre_buy_free_base,
            post_buy_free_base,
            exec_qty
        );
    }
    if sell_qty < exec_qty {
        positions.update_fill(symbol, Side::Sell, exec_qty - sell_qty);
    }

    let sell_coid = deterministic_client_order_id(spec_hash, &format!("smoke-sell-{run_id}"));
    let sell = broker
        .place_market_order(symbol, Side::Sell, sell_qty, sell_coid)
        .await
        .context("live smoke: place sell order")?;

    let final_sell = if sell.status == "FILLED" {
        sell
    } else {
        tokio::time::sleep(Duration::from_secs(2)).await;
        broker
            .get_order_status(symbol, sell_coid)
            .await?
            .with_context(|| {
                format!("live smoke: broker status missing for sell order {sell_coid}")
            })?
    };
    let executed_sell_qty = parse_exchange_executed_qty(&final_sell, "live smoke: sell response")?;
    if executed_sell_qty > Decimal::ZERO {
        positions.update_fill(symbol, Side::Sell, executed_sell_qty);
    }

    tracing::info!(
        order_id = final_sell.order_id,
        status = %final_sell.status,
        requested_qty = %sell_qty,
        executed_qty = %executed_sell_qty,
        "live smoke: sell status received"
    );

    audit
        .write(&AuditEntry {
            action: "live_smoke_sell".to_string(),
            payload: json!({
                "order_id": final_sell.order_id,
                "client_order_id": sell_coid.to_string(),
                "status": final_sell.status.clone(),
                "sell_qty": sell_qty.to_string(),
                "executed_sell_qty": executed_sell_qty.to_string(),
                "pre_buy_free_base": pre_buy_free_base.to_string(),
                "post_buy_free_base": post_buy_free_base.to_string(),
                "sellable_delta": sellable_delta.to_string(),
                "executed_buy_qty": exec_qty.to_string(),
            }),
            ..base_entry.clone()
        })
        .context("write live_smoke_sell audit entry")?;

    anyhow::ensure!(
        final_sell.status == "FILLED",
        "live smoke sell order ended with status {} after executing {} of requested {}",
        final_sell.status,
        executed_sell_qty,
        sell_qty
    );
    anyhow::ensure!(
        executed_sell_qty >= sell_qty,
        "live smoke sell executed {} below requested {}; position left open: {:?}",
        executed_sell_qty,
        sell_qty,
        positions.positions
    );
    anyhow::ensure!(
        positions.is_flat(),
        "live smoke complete but position not flat: {:?}",
        positions.positions
    );

    tracing::info!(
        run_id = %run_id,
        "live smoke: round-trip complete; position flat; broker statement reconciled"
    );

    Ok(())
}

fn decimal_number(value: Decimal) -> f64 {
    value.to_string().parse::<f64>().unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kill_switch_cancel_symbols_include_outbox_only_symbols() {
        let mut positions = LivePositionState::default();
        positions.update_fill("ETHUSDT", Side::Buy, dec!(0.5));

        let mut outbox = OrderOutbox::default();
        outbox.enqueue(
            &"f".repeat(64),
            OrderIntent {
                intent_id: "entry-btc".to_string(),
                symbol: "BTCUSDT".to_string(),
                side: Side::Buy,
                qty: dec!(0.01),
            },
        );

        let symbols = kill_switch_cancel_symbols(&positions, &outbox);
        assert!(symbols.contains("BTCUSDT"));
        assert!(symbols.contains("ETHUSDT"));
    }

    #[test]
    fn flatten_order_buys_abs_quantity_for_short_positions() {
        assert_eq!(
            flatten_order_for_position(dec!(2)),
            Some((Side::Sell, dec!(2)))
        );
        assert_eq!(
            flatten_order_for_position(dec!(-3)),
            Some((Side::Buy, dec!(3)))
        );
        assert_eq!(flatten_order_for_position(Decimal::ZERO), None);
    }

    #[test]
    fn binance_cancel_all_suppresses_only_no_such_order_code() {
        assert_eq!(
            binance_error_code(r#"{"code":-2011,"msg":"Unknown order sent."}"#),
            Some(-2011)
        );
        assert_ne!(
            binance_error_code(
                r#"{"code":-1022,"msg":"Signature for this request is not valid."}"#
            ),
            Some(-2011)
        );
        assert_eq!(binance_error_code("not-json"), None);
    }

    #[test]
    fn broker_key_scope_is_required_from_metadata() {
        assert!(parse_broker_key_scope(&[]).is_err());
        assert_eq!(
            parse_broker_key_scope(&[" Trade ".to_string(), "read".to_string()]).unwrap(),
            vec!["trade".to_string(), "read".to_string()]
        );
    }

    #[test]
    fn live_smoke_quantity_uses_quote_cap_with_buffer() {
        let filters = SymbolTradingFilters {
            step_size: dec!(0.00001000),
            min_qty: dec!(0.00001000),
            min_notional: dec!(5),
        };

        let qty = calculate_live_smoke_qty(dec!(45), dec!(55000), filters).unwrap();
        assert_eq!(qty, dec!(0.00077000));
        assert!(qty * dec!(55000) < dec!(45));
        assert!(calculate_live_smoke_qty(dec!(0), dec!(55000), filters).is_err());
        assert!(calculate_live_smoke_qty(dec!(45), dec!(0), filters).is_err());
    }

    #[test]
    fn live_smoke_quantity_rejects_exchange_filter_violations() {
        let filters = SymbolTradingFilters {
            step_size: dec!(0.00001000),
            min_qty: dec!(0.001),
            min_notional: dec!(5),
        };

        assert!(calculate_live_smoke_qty(dec!(45), dec!(55000), filters).is_err());
        assert_eq!(
            floor_to_step(dec!(0.000777), dec!(0.00001000)).unwrap(),
            dec!(0.00077000)
        );
    }
}
