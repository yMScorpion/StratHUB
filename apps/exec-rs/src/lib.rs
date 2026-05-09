use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Exchange {
    Binance,
    Bybit,
}

impl Exchange {
    pub fn primary_region(self) -> &'static str {
        match self {
            Self::Binance => "nrt",
            Self::Bybit => "sin",
        }
    }

    pub fn fallback_regions(self) -> &'static [&'static str] {
        match self {
            Self::Binance => &["sin", "hkg"],
            Self::Bybit => &["hkg", "nrt"],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecAlert {
    RateLimitExceeded {
        exchange: Exchange,
        retry_after_ms: Option<u64>,
    },
    ExchangeBan {
        exchange: Exchange,
        status: u16,
    },
    WsReconnectThrottled {
        exchange: Exchange,
    },
    ClockSkewExceeded {
        skew_ms: i64,
    },
    KillSwitchActive,
    RiskLimitExceeded {
        reason: String,
    },
}

#[derive(Debug)]
pub struct TokenBucket {
    exchange: Exchange,
    capacity: u32,
    available: u32,
    refill_per_second: u32,
    last_refill: Instant,
    used_weight: u64,
}

impl TokenBucket {
    pub fn new(exchange: Exchange, capacity: u32, refill_per_second: u32) -> Self {
        Self {
            exchange,
            capacity,
            available: capacity,
            refill_per_second,
            last_refill: Instant::now(),
            used_weight: 0,
        }
    }

    pub fn try_consume(&mut self, weight: u32) -> Result<(), ExecAlert> {
        self.refill();
        if self.available < weight {
            return Err(ExecAlert::RateLimitExceeded {
                exchange: self.exchange,
                retry_after_ms: None,
            });
        }
        self.available -= weight;
        self.used_weight += u64::from(weight);
        Ok(())
    }

    pub fn record_response(&self, status: u16, retry_after_ms: Option<u64>) -> Option<ExecAlert> {
        match status {
            429 => Some(ExecAlert::RateLimitExceeded {
                exchange: self.exchange,
                retry_after_ms,
            }),
            418 | 403 => Some(ExecAlert::ExchangeBan {
                exchange: self.exchange,
                status,
            }),
            _ => None,
        }
    }

    pub fn used_weight(&self) -> u64 {
        self.used_weight
    }

    fn refill(&mut self) {
        let elapsed = self.last_refill.elapsed().as_secs();
        if elapsed == 0 {
            return;
        }
        let refill = elapsed.saturating_mul(u64::from(self.refill_per_second));
        self.available = self.capacity.min(
            self.available
                .saturating_add(refill.min(u64::from(u32::MAX)) as u32),
        );
        self.last_refill = Instant::now();
    }
}

#[derive(Debug)]
pub struct WsReconnectThrottle {
    exchange: Exchange,
    max_attempts: usize,
    window: Duration,
    attempts: VecDeque<Instant>,
}

impl WsReconnectThrottle {
    pub fn new(exchange: Exchange, max_attempts: usize, window: Duration) -> Self {
        Self {
            exchange,
            max_attempts,
            window,
            attempts: VecDeque::new(),
        }
    }

    pub fn record_attempt(&mut self) -> Result<(), ExecAlert> {
        let now = Instant::now();
        while self
            .attempts
            .front()
            .is_some_and(|attempt| now.duration_since(*attempt) > self.window)
        {
            self.attempts.pop_front();
        }
        if self.attempts.len() >= self.max_attempts {
            return Err(ExecAlert::WsReconnectThrottled {
                exchange: self.exchange,
            });
        }
        self.attempts.push_back(now);
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct ClockSkewMonitor {
    max_skew_ms: i64,
}

impl ClockSkewMonitor {
    pub fn new(max_skew_ms: i64) -> Self {
        Self { max_skew_ms }
    }

    pub fn observe(
        &self,
        local: DateTime<Utc>,
        reference: DateTime<Utc>,
    ) -> Result<i64, ExecAlert> {
        let skew_ms = (local - reference).num_milliseconds().abs();
        if skew_ms > self.max_skew_ms {
            Err(ExecAlert::ClockSkewExceeded { skew_ms })
        } else {
            Ok(skew_ms)
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskLimits {
    pub kill_switch_active: bool,
    pub max_daily_loss_pct: Decimal,
    pub max_position_pct: Decimal,
    pub max_concurrent_orders: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskState {
    pub equity: Decimal,
    pub start_of_day_equity: Decimal,
    pub gross_position_pct: Decimal,
    pub open_orders: usize,
}

#[derive(Debug, Clone)]
pub struct RiskGuard {
    limits: RiskLimits,
}

impl RiskGuard {
    pub fn new(limits: RiskLimits) -> Self {
        Self { limits }
    }

    pub fn check(&self, state: &RiskState) -> Result<(), ExecAlert> {
        if self.limits.kill_switch_active {
            return Err(ExecAlert::KillSwitchActive);
        }
        if state.start_of_day_equity > Decimal::ZERO {
            let drawdown_pct = ((state.start_of_day_equity - state.equity)
                / state.start_of_day_equity)
                * dec!(100);
            if drawdown_pct > self.limits.max_daily_loss_pct {
                return Err(ExecAlert::RiskLimitExceeded {
                    reason: format!(
                        "daily loss {drawdown_pct}% exceeds {}%",
                        self.limits.max_daily_loss_pct
                    ),
                });
            }
        }
        if state.gross_position_pct > self.limits.max_position_pct {
            return Err(ExecAlert::RiskLimitExceeded {
                reason: format!(
                    "gross position {}% exceeds {}%",
                    state.gross_position_pct, self.limits.max_position_pct
                ),
            });
        }
        if state.open_orders >= self.limits.max_concurrent_orders {
            return Err(ExecAlert::RiskLimitExceeded {
                reason: "concurrent order cap reached".to_string(),
            });
        }
        Ok(())
    }
}

pub fn deterministic_client_order_id(spec_hash: &str, intent_id: &str) -> Uuid {
    Uuid::new_v5(
        &Uuid::NAMESPACE_OID,
        format!("{spec_hash}:{intent_id}").as_bytes(),
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Buy,
    Sell,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderIntent {
    pub intent_id: String,
    pub symbol: String,
    pub side: Side,
    pub qty: Decimal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutboxStatus {
    Pending,
    Acked,
    Filled,
    Rejected,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutboxOrder {
    pub client_order_id: Uuid,
    pub intent: OrderIntent,
    pub status: OutboxStatus,
}

#[derive(Debug, Default)]
pub struct OrderOutbox {
    orders: BTreeMap<Uuid, OutboxOrder>,
}

impl OrderOutbox {
    pub fn enqueue(&mut self, spec_hash: &str, intent: OrderIntent) -> Uuid {
        let client_order_id = deterministic_client_order_id(spec_hash, &intent.intent_id);
        self.orders.entry(client_order_id).or_insert(OutboxOrder {
            client_order_id,
            intent,
            status: OutboxStatus::Pending,
        });
        client_order_id
    }

    pub fn mark(&mut self, client_order_id: Uuid, status: OutboxStatus) -> bool {
        if let Some(order) = self.orders.get_mut(&client_order_id) {
            order.status = status;
            true
        } else {
            false
        }
    }

    pub fn pending_for_reconciliation(&self) -> Vec<Uuid> {
        self.orders
            .values()
            .filter(|order| matches!(order.status, OutboxStatus::Pending | OutboxStatus::Acked))
            .map(|order| order.client_order_id)
            .collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketTick {
    pub symbol: String,
    pub price: Decimal,
    pub ts: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fill {
    pub client_order_id: Uuid,
    pub symbol: String,
    pub side: Side,
    pub qty: Decimal,
    pub price: Decimal,
    pub ts: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct PaperBroker {
    slippage_bps: Decimal,
}

impl PaperBroker {
    pub fn new(slippage_bps: Decimal) -> Self {
        Self { slippage_bps }
    }

    pub fn execute_market(
        &self,
        client_order_id: Uuid,
        intent: &OrderIntent,
        tick: &MarketTick,
    ) -> Fill {
        let slip = self.slippage_bps / dec!(10000);
        let price = match intent.side {
            Side::Buy => tick.price * (Decimal::ONE + slip),
            Side::Sell => tick.price * (Decimal::ONE - slip),
        };
        Fill {
            client_order_id,
            symbol: intent.symbol.clone(),
            side: intent.side,
            qty: intent.qty,
            price,
            ts: tick.ts,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScorecardThresholds {
    pub min_trades: u32,
    pub max_drawdown_pct: Decimal,
    pub max_slippage_bps_vs_backtest: Decimal,
    pub max_risk_guard_events: u32,
    pub max_exec_errors: u32,
    pub require_walk_forward: bool,
    pub require_oos: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationMetrics {
    pub trades: u32,
    pub max_drawdown_pct: Decimal,
    pub slippage_bps_vs_backtest: Decimal,
    pub risk_guard_events: u32,
    pub exec_errors: u32,
    pub walk_forward_passed: bool,
    pub oos_passed: bool,
    pub run_days: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScorecardResult {
    pub passed: bool,
    pub reasons: Vec<String>,
}

pub fn evaluate_scorecard(
    thresholds: &ScorecardThresholds,
    metrics: &ValidationMetrics,
) -> ScorecardResult {
    let mut reasons = Vec::new();
    if metrics.run_days < 7 {
        reasons.push(format!("run lasted {} days; required 7", metrics.run_days));
    }
    if metrics.trades < thresholds.min_trades {
        reasons.push(format!(
            "{} trades observed; required at least {}",
            metrics.trades, thresholds.min_trades
        ));
    }
    if metrics.max_drawdown_pct > thresholds.max_drawdown_pct {
        reasons.push(format!(
            "max drawdown {}% exceeded {}%",
            metrics.max_drawdown_pct, thresholds.max_drawdown_pct
        ));
    }
    if metrics.slippage_bps_vs_backtest > thresholds.max_slippage_bps_vs_backtest {
        reasons.push(format!(
            "slippage {} bps exceeded {} bps",
            metrics.slippage_bps_vs_backtest, thresholds.max_slippage_bps_vs_backtest
        ));
    }
    if metrics.risk_guard_events > thresholds.max_risk_guard_events {
        reasons.push(format!(
            "{} risk-guard events exceeded {}",
            metrics.risk_guard_events, thresholds.max_risk_guard_events
        ));
    }
    if metrics.exec_errors > thresholds.max_exec_errors {
        reasons.push(format!(
            "{} execution errors exceeded {}",
            metrics.exec_errors, thresholds.max_exec_errors
        ));
    }
    if thresholds.require_walk_forward && !metrics.walk_forward_passed {
        reasons.push("walk-forward check failed".to_string());
    }
    if thresholds.require_oos && !metrics.oos_passed {
        reasons.push("out-of-sample check failed".to_string());
    }
    ScorecardResult {
        passed: reasons.is_empty(),
        reasons,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MachineSpec {
    pub user_id: String,
    pub strategy_id: String,
    pub validation_id: String,
    pub exchange: Exchange,
    pub monthly_budget_cents: u32,
    pub already_spent_cents: u32,
    pub estimated_cost_cents: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MachinePlan {
    pub lease_key: String,
    pub region: String,
    pub fallback_regions: Vec<String>,
    pub labels: BTreeMap<String, String>,
}

pub fn plan_fly_machine(spec: &MachineSpec, ttl_hours: u32) -> Result<MachinePlan, String> {
    if spec
        .already_spent_cents
        .saturating_add(spec.estimated_cost_cents)
        > spec.monthly_budget_cents
    {
        return Err("hard per-user validator budget cap exceeded".to_string());
    }

    let mut labels = BTreeMap::new();
    labels.insert("user_id".to_string(), spec.user_id.clone());
    labels.insert("strategy_id".to_string(), spec.strategy_id.clone());
    labels.insert("validation_id".to_string(), spec.validation_id.clone());
    labels.insert("ttl_hours".to_string(), ttl_hours.to_string());
    labels.insert("mode".to_string(), "paper".to_string());

    Ok(MachinePlan {
        lease_key: format!("validation:{}", spec.validation_id),
        region: spec.exchange.primary_region().to_string(),
        fallback_regions: spec
            .exchange
            .fallback_regions()
            .iter()
            .map(|region| (*region).to_string())
            .collect(),
        labels,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MachineInventoryRow {
    pub id: String,
    pub validation_id: String,
    pub expires_at: DateTime<Utc>,
    pub leased: bool,
}

pub fn janitor_destroy_list(
    machines: &[MachineInventoryRow],
    active_validation_ids: &BTreeSet<String>,
    now: DateTime<Utc>,
) -> Vec<String> {
    machines
        .iter()
        .filter(|machine| {
            machine.expires_at <= now || !active_validation_ids.contains(&machine.validation_id)
        })
        .map(|machine| machine.id.clone())
        .collect()
}

// ---- Phase 6: Live Mode + Safeguards ----

impl Exchange {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Binance => "binance",
            Self::Bybit => "bybit",
        }
    }
}

impl Side {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Buy => "BUY",
            Self::Sell => "SELL",
        }
    }
}

// KMS envelope decryption (AES-256-GCM).
// In production: cloud KMS decrypts the DEK; the DEK decrypts the broker key.
// The executor receives only dek_hex (from KMS.Decrypt), nonce_hex, and ciphertext_hex.

#[derive(Debug, thiserror::Error)]
pub enum KmsError {
    #[error("invalid DEK length: expected 32 bytes")]
    InvalidDekLength,
    #[error("invalid nonce length: expected 12 bytes")]
    InvalidNonceLength,
    #[error("hex decode error: {0}")]
    HexDecode(String),
    #[error("decryption failed: authentication tag mismatch or corrupted ciphertext")]
    DecryptionFailed,
    #[error("decrypted key is not valid UTF-8")]
    InvalidUtf8,
    #[error("scope violation: {0}")]
    ScopeViolation(String),
}

pub fn decrypt_broker_key(
    dek_hex: &str,
    nonce_hex: &str,
    ciphertext_hex: &str,
) -> Result<String, KmsError> {
    use aes_gcm::{
        aead::{Aead, KeyInit},
        Aes256Gcm, Key, Nonce,
    };

    let dek = hex::decode(dek_hex).map_err(|e| KmsError::HexDecode(e.to_string()))?;
    let nonce_bytes = hex::decode(nonce_hex).map_err(|e| KmsError::HexDecode(e.to_string()))?;
    let ciphertext = hex::decode(ciphertext_hex).map_err(|e| KmsError::HexDecode(e.to_string()))?;

    if dek.len() != 32 {
        return Err(KmsError::InvalidDekLength);
    }
    if nonce_bytes.len() != 12 {
        return Err(KmsError::InvalidNonceLength);
    }

    let key = Key::<Aes256Gcm>::from_slice(&dek);
    let cipher = Aes256Gcm::new(key);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let plaintext = cipher
        .decrypt(nonce, ciphertext.as_ref())
        .map_err(|_| KmsError::DecryptionFailed)?;

    String::from_utf8(plaintext).map_err(|_| KmsError::InvalidUtf8)
}

pub fn encrypt_broker_key(
    dek_hex: &str,
    nonce_hex: &str,
    plaintext: &str,
) -> Result<String, KmsError> {
    use aes_gcm::{
        aead::{Aead, KeyInit},
        Aes256Gcm, Key, Nonce,
    };

    let dek = hex::decode(dek_hex).map_err(|e| KmsError::HexDecode(e.to_string()))?;
    let nonce_bytes = hex::decode(nonce_hex).map_err(|e| KmsError::HexDecode(e.to_string()))?;

    if dek.len() != 32 {
        return Err(KmsError::InvalidDekLength);
    }
    if nonce_bytes.len() != 12 {
        return Err(KmsError::InvalidNonceLength);
    }

    let key = Key::<Aes256Gcm>::from_slice(&dek);
    let cipher = Aes256Gcm::new(key);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_bytes())
        .map_err(|_| KmsError::DecryptionFailed)?;

    Ok(hex::encode(ciphertext))
}

// Broker credentials: loaded at process start after KMS decrypt; never persisted in plaintext.

#[derive(Debug, Clone)]
pub struct BrokerCredentials {
    pub api_key: String,
    pub api_secret: String,
    pub scope: Vec<String>,
}

impl BrokerCredentials {
    pub fn verify_scope(&self) -> Result<(), KmsError> {
        if !self.scope.iter().any(|s| s == "trade") {
            return Err(KmsError::ScopeViolation("trade scope missing".to_string()));
        }
        if self.scope.iter().any(|s| s == "withdraw") {
            return Err(KmsError::ScopeViolation(
                "withdraw scope must not be granted on live keys".to_string(),
            ));
        }
        Ok(())
    }
}

// Jurisdiction + ToS + risk-ack gate. All three must be present before "Go Live" is offered.

#[derive(Debug, thiserror::Error)]
pub enum ComplianceError {
    #[error("jurisdiction not recorded; complete profile settings before going live")]
    NoJurisdiction,
    #[error("terms of service not accepted")]
    TosNotAccepted,
    #[error("risk acknowledgement not completed")]
    RiskAckMissing,
}

#[derive(Debug, Clone)]
pub struct ComplianceRecord {
    pub user_id: String,
    pub jurisdiction: Option<String>,
    pub tos_accepted_at: Option<DateTime<Utc>>,
    pub risk_ack_at: Option<DateTime<Utc>>,
}

pub fn check_compliance_gate(record: &ComplianceRecord) -> Result<(), ComplianceError> {
    if record.jurisdiction.is_none() {
        return Err(ComplianceError::NoJurisdiction);
    }
    if record.tos_accepted_at.is_none() {
        return Err(ComplianceError::TosNotAccepted);
    }
    if record.risk_ack_at.is_none() {
        return Err(ComplianceError::RiskAckMissing);
    }
    Ok(())
}

// Hold-to-confirm: token = sha256(spec_hash:account_id:exchange).
// Pre-computed by the API layer and passed as EXEC_CONFIRM_TOKEN. Binding the
// token to spec_hash + account_id + exchange means any parameter change invalidates it.

pub struct ConfirmationGate;

impl ConfirmationGate {
    pub fn expected_token(spec_hash: &str, account_id: &str, exchange: Exchange) -> String {
        use sha2::{Digest, Sha256};
        let input = format!("{spec_hash}:{account_id}:{}", exchange.as_str());
        hex::encode(Sha256::digest(input.as_bytes()))
    }

    pub fn verify(provided: &str, spec_hash: &str, account_id: &str, exchange: Exchange) -> bool {
        provided == Self::expected_token(spec_hash, account_id, exchange)
    }
}

// Append-only audit log: one JSONL entry per live action.
// Written to a local file; flushed on every write so a crash loses at most one entry.

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    pub ts: DateTime<Utc>,
    pub user_id: String,
    pub run_id: String,
    pub spec_hash: String,
    pub exchange: Exchange,
    pub action: String,
    pub payload: serde_json::Value,
    pub result: String,
}

pub struct AuditLog {
    path: std::path::PathBuf,
}

impl AuditLog {
    pub fn new(dir: &std::path::Path, run_id: &str) -> Self {
        Self {
            path: dir.join(format!("{run_id}-audit.jsonl")),
        }
    }

    pub fn write(&self, entry: &AuditEntry) -> Result<(), std::io::Error> {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        writeln!(f, "{}", serde_json::to_string(entry).unwrap())?;
        Ok(())
    }

    pub fn path(&self) -> &std::path::Path {
        &self.path
    }
}

// Live position state: signed qty per symbol (+ = long, - = short).

#[derive(Debug, Default, Clone)]
pub struct LivePositionState {
    pub positions: BTreeMap<String, Decimal>,
    pub equity: Decimal,
    pub start_of_day_equity: Decimal,
}

impl LivePositionState {
    pub fn update_fill(&mut self, symbol: &str, side: Side, qty: Decimal) {
        let entry = self.positions.entry(symbol.to_string()).or_default();
        match side {
            Side::Buy => *entry += qty,
            Side::Sell => *entry -= qty,
        }
        if *entry == Decimal::ZERO {
            self.positions.remove(symbol);
        }
    }

    pub fn open_symbols(&self) -> Vec<String> {
        self.positions.keys().cloned().collect()
    }

    pub fn is_flat(&self) -> bool {
        self.positions.is_empty()
    }

    pub fn gross_position_pct(&self) -> Decimal {
        if self.equity.is_zero() {
            return Decimal::ZERO;
        }
        let gross: Decimal = self.positions.values().map(|q| q.abs()).sum();
        (gross / self.equity) * dec!(100)
    }
}

// Local risk cache: used when DB/API is unreachable. Fails closed (activates kill-switch)
// after `stale_after_secs` without a successful refresh.

#[derive(Debug, Clone)]
pub struct LocalRiskCache {
    pub limits: RiskLimits,
    pub last_updated: DateTime<Utc>,
    pub stale_after_secs: u64,
}

impl LocalRiskCache {
    pub fn new(limits: RiskLimits, stale_after_secs: u64) -> Self {
        Self {
            limits,
            last_updated: Utc::now(),
            stale_after_secs,
        }
    }

    pub fn is_stale(&self) -> bool {
        let age_secs = (Utc::now() - self.last_updated).num_seconds().max(0) as u64;
        age_secs > self.stale_after_secs
    }

    pub fn effective_limits(&self) -> RiskLimits {
        if self.is_stale() {
            RiskLimits {
                kill_switch_active: true,
                ..self.limits.clone()
            }
        } else {
            self.limits.clone()
        }
    }
}

// Kill-switch action spec: always cancels all open orders; optionally flattens positions.

#[derive(Debug, Clone)]
pub struct KillSwitchAction {
    pub cancel_all: bool,
    pub flatten_positions: bool,
    pub triggered_at: DateTime<Utc>,
    pub reason: String,
}

impl KillSwitchAction {
    pub fn new(flatten: bool, reason: impl Into<String>) -> Self {
        Self {
            cancel_all: true,
            flatten_positions: flatten,
            triggered_at: Utc::now(),
            reason: reason.into(),
        }
    }
}

// Fill reconciliation: compares what the local outbox recorded against the exchange report.

#[derive(Debug, Clone)]
pub struct ReconciliationCheck {
    pub client_order_id: Uuid,
    pub local_symbol: String,
    pub local_side: Side,
    pub local_qty: Decimal,
    pub local_price: Decimal,
    pub exchange_symbol: String,
    pub exchange_side: Side,
    pub exchange_qty: Decimal,
    pub exchange_price: Decimal,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ReconciliationResult {
    Matched,
    SymbolMismatch {
        local: String,
        exchange: String,
    },
    SideMismatch {
        local: Side,
        exchange: Side,
    },
    QtyMismatch {
        local: Decimal,
        exchange: Decimal,
        diff: Decimal,
    },
    PriceDriftExcessive {
        diff_bps: Decimal,
    },
}

impl ReconciliationResult {
    pub fn is_matched(&self) -> bool {
        matches!(self, Self::Matched)
    }
}

pub fn reconcile_fill(
    check: &ReconciliationCheck,
    max_price_diff_bps: Decimal,
) -> ReconciliationResult {
    if check.local_symbol != check.exchange_symbol {
        return ReconciliationResult::SymbolMismatch {
            local: check.local_symbol.clone(),
            exchange: check.exchange_symbol.clone(),
        };
    }
    if check.local_side != check.exchange_side {
        return ReconciliationResult::SideMismatch {
            local: check.local_side,
            exchange: check.exchange_side,
        };
    }
    let qty_diff = (check.local_qty - check.exchange_qty).abs();
    if qty_diff > Decimal::ZERO {
        return ReconciliationResult::QtyMismatch {
            local: check.local_qty,
            exchange: check.exchange_qty,
            diff: qty_diff,
        };
    }
    if check.exchange_price > Decimal::ZERO {
        let diff_bps =
            ((check.local_price - check.exchange_price).abs() / check.exchange_price) * dec!(10000);
        if diff_bps > max_price_diff_bps {
            return ReconciliationResult::PriceDriftExcessive { diff_bps };
        }
    }
    ReconciliationResult::Matched
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_order_id_is_deterministic_uuid_v5() {
        let a = deterministic_client_order_id(&"a".repeat(64), "entry-1");
        let b = deterministic_client_order_id(&"a".repeat(64), "entry-1");
        let c = deterministic_client_order_id(&"a".repeat(64), "entry-2");
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn risk_guard_fails_closed_on_kill_switch() {
        let guard = RiskGuard::new(RiskLimits {
            kill_switch_active: true,
            max_daily_loss_pct: dec!(2),
            max_position_pct: dec!(5),
            max_concurrent_orders: 5,
        });
        let state = RiskState {
            equity: dec!(1000),
            start_of_day_equity: dec!(1000),
            gross_position_pct: dec!(0),
            open_orders: 0,
        };
        assert!(matches!(
            guard.check(&state),
            Err(ExecAlert::KillSwitchActive)
        ));
    }

    #[test]
    fn outbox_is_idempotent_and_reconciles_open_orders() {
        let mut outbox = OrderOutbox::default();
        let intent = OrderIntent {
            intent_id: "entry-1".to_string(),
            symbol: "BTCUSDT".to_string(),
            side: Side::Buy,
            qty: dec!(0.01),
        };
        let first = outbox.enqueue(&"f".repeat(64), intent.clone());
        let second = outbox.enqueue(&"f".repeat(64), intent);
        assert_eq!(first, second);
        assert_eq!(outbox.pending_for_reconciliation(), vec![first]);
        assert!(outbox.mark(first, OutboxStatus::Filled));
        assert!(outbox.pending_for_reconciliation().is_empty());
    }

    #[test]
    fn token_bucket_tracks_weight_and_alerts_on_exchange_limits() {
        let mut bucket = TokenBucket::new(Exchange::Binance, 10, 1);
        bucket.try_consume(7).unwrap();
        assert_eq!(bucket.used_weight(), 7);
        assert!(bucket.try_consume(4).is_err());
        assert!(matches!(
            bucket.record_response(418, None),
            Some(ExecAlert::ExchangeBan { status: 418, .. })
        ));
    }

    #[test]
    fn paper_broker_uses_virtual_fill_with_live_tick_price() {
        let broker = PaperBroker::new(dec!(10));
        let intent = OrderIntent {
            intent_id: "entry".to_string(),
            symbol: "BTCUSDT".to_string(),
            side: Side::Buy,
            qty: dec!(1),
        };
        let tick = MarketTick {
            symbol: "BTCUSDT".to_string(),
            price: dec!(100),
            ts: Utc::now(),
        };
        let fill = broker.execute_market(Uuid::nil(), &intent, &tick);
        assert_eq!(fill.price, dec!(100.100));
    }

    #[test]
    fn scorecard_requires_more_than_positive_pnl() {
        let thresholds = ScorecardThresholds {
            min_trades: 5,
            max_drawdown_pct: dec!(8),
            max_slippage_bps_vs_backtest: dec!(20),
            max_risk_guard_events: 0,
            max_exec_errors: 0,
            require_walk_forward: true,
            require_oos: true,
        };
        let metrics = ValidationMetrics {
            trades: 3,
            max_drawdown_pct: dec!(2),
            slippage_bps_vs_backtest: dec!(5),
            risk_guard_events: 0,
            exec_errors: 0,
            walk_forward_passed: true,
            oos_passed: true,
            run_days: 7,
        };
        let result = evaluate_scorecard(&thresholds, &metrics);
        assert!(!result.passed);
        assert!(result.reasons[0].contains("3 trades"));
    }

    #[test]
    fn fly_machine_plan_pins_region_and_enforces_budget() {
        let spec = MachineSpec {
            user_id: "u1".to_string(),
            strategy_id: "s1".to_string(),
            validation_id: "v1".to_string(),
            exchange: Exchange::Binance,
            monthly_budget_cents: 1000,
            already_spent_cents: 200,
            estimated_cost_cents: 300,
        };
        let plan = plan_fly_machine(&spec, 168).unwrap();
        assert_eq!(plan.region, "nrt");
        assert_eq!(plan.labels["mode"], "paper");

        let over_budget = MachineSpec {
            already_spent_cents: 900,
            ..spec
        };
        assert!(plan_fly_machine(&over_budget, 168).is_err());
    }

    #[test]
    fn kms_encrypt_decrypt_round_trip() {
        let dek_hex = "00".repeat(32);
        let nonce_hex = "00".repeat(12);
        let plaintext = "LIVE_API_KEY_abc123XYZ";

        let ciphertext_hex = encrypt_broker_key(&dek_hex, &nonce_hex, plaintext).unwrap();
        let recovered = decrypt_broker_key(&dek_hex, &nonce_hex, &ciphertext_hex).unwrap();
        assert_eq!(recovered, plaintext);
    }

    #[test]
    fn kms_decrypt_wrong_tag_fails_closed() {
        let dek_hex = "00".repeat(32);
        let nonce_hex = "00".repeat(12);
        let plaintext = "secret-api-key";
        let ciphertext_hex = encrypt_broker_key(&dek_hex, &nonce_hex, plaintext).unwrap();

        // corrupt one byte of the ciphertext
        let mut bytes = hex::decode(&ciphertext_hex).unwrap();
        bytes[0] ^= 0xFF;
        let bad_hex = hex::encode(bytes);

        assert!(matches!(
            decrypt_broker_key(&dek_hex, &nonce_hex, &bad_hex),
            Err(KmsError::DecryptionFailed)
        ));
    }

    #[test]
    fn broker_credentials_enforce_trade_only_scope() {
        let trade_only = BrokerCredentials {
            api_key: "key".to_string(),
            api_secret: "secret".to_string(),
            scope: vec!["trade".to_string()],
        };
        assert!(trade_only.verify_scope().is_ok());

        let with_withdraw = BrokerCredentials {
            scope: vec!["trade".to_string(), "withdraw".to_string()],
            ..trade_only.clone()
        };
        assert!(matches!(
            with_withdraw.verify_scope(),
            Err(KmsError::ScopeViolation(_))
        ));

        let no_trade = BrokerCredentials {
            scope: vec!["read".to_string()],
            ..trade_only
        };
        assert!(matches!(
            no_trade.verify_scope(),
            Err(KmsError::ScopeViolation(_))
        ));
    }

    #[test]
    fn compliance_gate_requires_jurisdiction_tos_and_risk_ack() {
        let valid = ComplianceRecord {
            user_id: "u1".to_string(),
            jurisdiction: Some("US".to_string()),
            tos_accepted_at: Some(Utc::now()),
            risk_ack_at: Some(Utc::now()),
        };
        assert!(check_compliance_gate(&valid).is_ok());

        let no_jurisdiction = ComplianceRecord {
            jurisdiction: None,
            ..valid.clone()
        };
        assert!(matches!(
            check_compliance_gate(&no_jurisdiction),
            Err(ComplianceError::NoJurisdiction)
        ));

        let no_tos = ComplianceRecord {
            tos_accepted_at: None,
            ..valid.clone()
        };
        assert!(matches!(
            check_compliance_gate(&no_tos),
            Err(ComplianceError::TosNotAccepted)
        ));

        let no_risk_ack = ComplianceRecord {
            risk_ack_at: None,
            ..valid
        };
        assert!(matches!(
            check_compliance_gate(&no_risk_ack),
            Err(ComplianceError::RiskAckMissing)
        ));
    }

    #[test]
    fn confirmation_gate_binds_to_spec_hash_account_and_exchange() {
        let spec_hash = "a".repeat(64);
        let account_id = "sub-account-50usd";

        let token = ConfirmationGate::expected_token(&spec_hash, account_id, Exchange::Binance);
        assert!(ConfirmationGate::verify(
            &token,
            &spec_hash,
            account_id,
            Exchange::Binance
        ));

        // Wrong token
        assert!(!ConfirmationGate::verify(
            "wrong",
            &spec_hash,
            account_id,
            Exchange::Binance
        ));
        // Different spec_hash
        assert!(!ConfirmationGate::verify(
            &token,
            &"b".repeat(64),
            account_id,
            Exchange::Binance
        ));
        // Different exchange
        assert!(!ConfirmationGate::verify(
            &token,
            &spec_hash,
            account_id,
            Exchange::Bybit
        ));
    }

    #[test]
    fn audit_log_appends_jsonl_entries() {
        use std::io::BufRead;
        let dir = std::env::temp_dir().join("exec-rs-phase6-audit-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let log = AuditLog::new(&dir, "run-0001");
        let entry = AuditEntry {
            ts: Utc::now(),
            user_id: "u1".to_string(),
            run_id: "run-0001".to_string(),
            spec_hash: "a".repeat(64),
            exchange: Exchange::Binance,
            action: "go_live".to_string(),
            payload: serde_json::json!({"dry_run": false}),
            result: "ok".to_string(),
        };

        log.write(&entry).unwrap();
        log.write(&AuditEntry {
            action: "kill_switch".to_string(),
            ..entry
        })
        .unwrap();

        let file = std::fs::File::open(log.path()).unwrap();
        let lines: Vec<String> = std::io::BufReader::new(file)
            .lines()
            .map_while(Result::ok)
            .collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("go_live"));
        assert!(lines[1].contains("kill_switch"));
    }

    #[test]
    fn local_risk_cache_fails_closed_when_stale() {
        let limits = RiskLimits {
            kill_switch_active: false,
            max_daily_loss_pct: dec!(2),
            max_position_pct: dec!(5),
            max_concurrent_orders: 5,
        };

        let fresh = LocalRiskCache::new(limits.clone(), 300);
        assert!(!fresh.is_stale());
        assert!(!fresh.effective_limits().kill_switch_active);

        let mut stale = LocalRiskCache::new(limits, 0);
        stale.last_updated = Utc::now() - chrono::Duration::seconds(10);
        assert!(stale.is_stale());
        assert!(stale.effective_limits().kill_switch_active);
    }

    #[test]
    fn live_position_tracks_fills_and_reports_flat() {
        let mut pos = LivePositionState::default();
        assert!(pos.is_flat());

        pos.update_fill("BTCUSDT", Side::Buy, dec!(0.01));
        assert!(!pos.is_flat());
        assert_eq!(pos.positions["BTCUSDT"], dec!(0.01));

        pos.update_fill("BTCUSDT", Side::Sell, dec!(0.01));
        assert!(pos.is_flat());
    }

    #[test]
    fn kill_switch_action_always_includes_cancel_all() {
        let with_flatten = KillSwitchAction::new(true, "signal");
        assert!(with_flatten.cancel_all);
        assert!(with_flatten.flatten_positions);

        let no_flatten = KillSwitchAction::new(false, "signal");
        assert!(no_flatten.cancel_all);
        assert!(!no_flatten.flatten_positions);
    }

    #[test]
    fn reconcile_fill_detects_all_mismatch_kinds() {
        let base = ReconciliationCheck {
            client_order_id: Uuid::nil(),
            local_symbol: "BTCUSDT".to_string(),
            local_side: Side::Buy,
            local_qty: dec!(0.001),
            local_price: dec!(50000),
            exchange_symbol: "BTCUSDT".to_string(),
            exchange_side: Side::Buy,
            exchange_qty: dec!(0.001),
            exchange_price: dec!(50000),
        };

        assert!(reconcile_fill(&base, dec!(50)).is_matched());

        let sym_mismatch = ReconciliationCheck {
            exchange_symbol: "ETHUSDT".to_string(),
            ..base.clone()
        };
        assert!(matches!(
            reconcile_fill(&sym_mismatch, dec!(50)),
            ReconciliationResult::SymbolMismatch { .. }
        ));

        let side_mismatch = ReconciliationCheck {
            exchange_side: Side::Sell,
            ..base.clone()
        };
        assert!(matches!(
            reconcile_fill(&side_mismatch, dec!(50)),
            ReconciliationResult::SideMismatch { .. }
        ));

        let qty_mismatch = ReconciliationCheck {
            exchange_qty: dec!(0.002),
            ..base.clone()
        };
        assert!(matches!(
            reconcile_fill(&qty_mismatch, dec!(50)),
            ReconciliationResult::QtyMismatch { .. }
        ));

        // ~20000 bps price drift (50000 vs 60000)
        let price_drift = ReconciliationCheck {
            local_price: dec!(60000),
            ..base.clone()
        };
        assert!(matches!(
            reconcile_fill(&price_drift, dec!(50)),
            ReconciliationResult::PriceDriftExcessive { .. }
        ));
    }

    #[test]
    fn janitor_destroys_expired_or_orphaned_machines() {
        let now = Utc::now();
        let machines = vec![
            MachineInventoryRow {
                id: "expired".to_string(),
                validation_id: "active".to_string(),
                expires_at: now - chrono::Duration::seconds(1),
                leased: false,
            },
            MachineInventoryRow {
                id: "orphan".to_string(),
                validation_id: "missing".to_string(),
                expires_at: now + chrono::Duration::hours(1),
                leased: false,
            },
        ];
        let active = BTreeSet::from(["active".to_string()]);
        assert_eq!(
            janitor_destroy_list(&machines, &active, now),
            vec!["expired".to_string(), "orphan".to_string()]
        );
    }
}
