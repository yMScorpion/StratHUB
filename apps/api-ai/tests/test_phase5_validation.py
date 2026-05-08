from __future__ import annotations

import datetime as dt
from unittest.mock import MagicMock

from fastapi import status
from fastapi.testclient import TestClient

from api_ai.main import create_app

HEADERS = {
    "X-User-Id": "00000000-0000-0000-0000-000000000001",
    "X-Internal-Token": "replace-me",
}


def _client(sb_mock: MagicMock | None = None) -> TestClient:
    app = create_app()
    app.state.supabase = sb_mock or MagicMock()
    return TestClient(app)


def test_event_ingest_batches_and_updates_heartbeat() -> None:
    sb = MagicMock()
    sb.table.return_value.insert.return_value.execute.return_value = MagicMock(data=[])
    sb.table.return_value.upsert.return_value.execute.return_value = MagicMock(data=[])

    r = _client(sb).post(
        "/events/ingest",
        headers={"X-Internal-Token": "replace-me"},
        json={
            "validation_id": "val-1",
            "run_id": "run-1",
            "events": [
                {
                    "seq": 1,
                    "kind": "heartbeat",
                    "ts": "2026-05-07T00:00:00Z",
                    "payload": {"clock_skew_ms": 8},
                },
                {
                    "seq": 2,
                    "kind": "fill",
                    "ts": "2026-05-07T00:00:01Z",
                    "payload": {"client_order_id": "coid"},
                },
            ],
        },
    )

    assert r.status_code == status.HTTP_202_ACCEPTED, r.text
    assert r.json() == {"accepted": 2, "last_seq": 2}
    sb.table.assert_any_call("validation_events")
    sb.table.assert_any_call("validation_heartbeats")


def test_scorecard_explains_failure_not_pnl_only() -> None:
    r = _client().post(
        "/validations/val-1/scorecard",
        headers={"X-Internal-Token": "replace-me"},
        json={
            "thresholds": {"min_trades": 5},
            "metrics": {
                "trades": 2,
                "max_drawdown_pct": 1.0,
                "slippage_bps_vs_backtest": 3.0,
                "risk_guard_events": 0,
                "exec_errors": 0,
                "walk_forward_passed": True,
                "oos_passed": True,
                "run_days": 7,
            },
        },
    )

    assert r.status_code == status.HTTP_200_OK, r.text
    body = r.json()
    assert body["status"] == "failed"
    assert body["passed"] is False
    assert "2 trades observed" in body["reasons"][0]


def test_provision_validation_pins_region_and_returns_ttl_labels() -> None:
    sb = MagicMock()
    query = sb.table.return_value.select.return_value.eq.return_value
    query.in_.return_value.execute.return_value = MagicMock(count=0)
    query.eq.return_value.limit.return_value.execute.return_value = MagicMock(data=[])
    query.gte.return_value.lt.return_value.execute.return_value = MagicMock(data=[])
    sb.table.return_value.insert.return_value.execute.return_value = MagicMock(
        data=[{"id": "val-1", "status": "provisioning"}]
    )

    r = _client(sb).post(
        "/validations",
        headers=HEADERS,
        json={
            "strategy_id": "strategy-1",
            "spec_hash": "a" * 64,
            "exchange": "binance",
            "idempotency_key": "idem-1",
        },
    )

    assert r.status_code == status.HTTP_201_CREATED, r.text
    machine = r.json()["machine"]
    assert machine["region"] == "nrt"
    assert machine["fallback_regions"] == ["sin", "hkg"]
    assert machine["labels"]["mode"] == "paper"
    assert machine["ttl_expires_at"]


def test_provision_validation_idempotent_retry_bypasses_concurrency_cap() -> None:
    sb = MagicMock()
    query = sb.table.return_value.select.return_value.eq.return_value
    query.eq.return_value.limit.return_value.execute.return_value = MagicMock(
        data=[
            {
                "id": "val-1",
                "status": "running",
                "fly_region": "nrt",
                "fly_fallback_regions": ["sin", "hkg"],
                "lease_key": "validation:user:idem-1",
                "ttl_expires_at": "2026-05-14T00:00:00+00:00",
                "labels": {"mode": "paper"},
            }
        ]
    )
    query.in_.return_value.execute.return_value = MagicMock(count=99)

    r = _client(sb).post(
        "/validations",
        headers=HEADERS,
        json={
            "strategy_id": "strategy-1",
            "spec_hash": "a" * 64,
            "exchange": "binance",
            "idempotency_key": "idem-1",
        },
    )

    assert r.status_code == status.HTTP_201_CREATED, r.text
    assert r.json()["validation_id"] == "val-1"
    assert r.json()["status"] == "running"
    sb.table.return_value.insert.assert_not_called()


def test_provision_validation_monthly_budget_uses_current_period() -> None:
    sb = MagicMock()
    query = sb.table.return_value.select.return_value.eq.return_value
    query.eq.return_value.limit.return_value.execute.return_value = MagicMock(data=[])
    query.in_.return_value.execute.return_value = MagicMock(count=0)
    query.gte.return_value.lt.return_value.execute.return_value = MagicMock(
        data=[{"estimated_cost_cents": 200}]
    )
    sb.table.return_value.insert.return_value.execute.return_value = MagicMock(
        data=[{"id": "val-1", "status": "provisioning"}]
    )

    r = _client(sb).post(
        "/validations",
        headers=HEADERS,
        json={
            "strategy_id": "strategy-1",
            "spec_hash": "a" * 64,
            "exchange": "binance",
            "idempotency_key": "idem-1",
        },
    )

    assert r.status_code == status.HTTP_201_CREATED, r.text
    budget_query = sb.table.return_value.select.return_value.eq.return_value.gte.return_value
    budget_query.lt.assert_called_once()


def test_janitor_marks_orphaned_and_expired_machines() -> None:
    now = dt.datetime(2026, 5, 7, tzinfo=dt.UTC)
    r = _client().post(
        "/validations/janitor/destroy-list",
        headers={"X-Internal-Token": "replace-me"},
        json={
            "now": now.isoformat(),
            "active_validation_ids": ["active"],
            "machines": [
                {
                    "id": "expired",
                    "validation_id": "active",
                    "expires_at": (now - dt.timedelta(seconds=1)).isoformat(),
                },
                {
                    "id": "orphan",
                    "validation_id": "missing",
                    "expires_at": (now + dt.timedelta(hours=1)).isoformat(),
                },
            ],
        },
    )

    assert r.status_code == status.HTTP_200_OK, r.text
    assert r.json()["destroy_machine_ids"] == ["expired", "orphan"]
