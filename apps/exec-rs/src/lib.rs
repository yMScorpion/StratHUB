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
