# Broker Outage Runbook

## Trigger

- Exchange 4xx/5xx rate exceeds alert threshold.
- WebSocket market data or account stream is stale.
- Exchange status page reports degraded trading.

## Procedure

1. Pause new live and paper launches for the affected exchange and region.
2. Switch active live runs to fail-closed mode.
3. Cancel stale open orders when REST cancel endpoints are healthy.
4. If cancel endpoints are unhealthy, stop submitting new orders and poll reconciliation until status is known.
5. Notify affected users with exchange, region, run ids, and current exposure.

## Recovery

- Require five clean minutes of REST and WebSocket health before unpausing.
- Reconcile fills against broker statements before allowing promotion or restart.
