"""Validation run provisioning, scorecard grading, and janitor helpers."""

from __future__ import annotations

import datetime as dt
from typing import Any, Literal

from fastapi import APIRouter, Depends, HTTPException, Request, status
from pydantic import BaseModel, Field

from .jobs import _user_id

router = APIRouter(prefix="/validations", tags=["validations"])

EXCHANGE_REGIONS = {
    "binance": ("nrt", ["sin", "hkg"]),
    "bybit": ("sin", ["hkg", "nrt"]),
}
VALIDATION_DAYS_REQUIRED = 7


class ScorecardThresholds(BaseModel):
    min_trades: int = Field(default=10, ge=0)
    max_drawdown_pct: float = Field(default=8.0, ge=0)
    max_slippage_bps_vs_backtest: float = Field(default=25.0, ge=0)
    max_risk_guard_events: int = Field(default=0, ge=0)
    max_exec_errors: int = Field(default=0, ge=0)
    require_walk_forward: bool = True
    require_oos: bool = True


class ValidationMetrics(BaseModel):
    trades: int = Field(ge=0)
    max_drawdown_pct: float = Field(ge=0)
    slippage_bps_vs_backtest: float = Field(ge=0)
    risk_guard_events: int = Field(ge=0)
    exec_errors: int = Field(ge=0)
    walk_forward_passed: bool
    oos_passed: bool
    run_days: int = Field(ge=0)


class ScorecardBody(BaseModel):
    thresholds: ScorecardThresholds = Field(default_factory=ScorecardThresholds)
    metrics: ValidationMetrics


class ProvisionValidationBody(BaseModel):
    strategy_id: str = Field(min_length=1, max_length=128)
    spec_hash: str = Field(min_length=64, max_length=64, pattern=r"^[0-9a-f]{64}$")
    exchange: Literal["binance", "bybit"] = "binance"
    idempotency_key: str = Field(min_length=1, max_length=128)


class MachineInventoryRow(BaseModel):
    id: str
    validation_id: str
    expires_at: dt.datetime
    leased: bool = False


class JanitorBody(BaseModel):
    machines: list[MachineInventoryRow]
    active_validation_ids: set[str] = Field(default_factory=set)
    now: dt.datetime | None = None


def evaluate_scorecard(
    thresholds: ScorecardThresholds, metrics: ValidationMetrics
) -> dict[str, Any]:
    reasons: list[str] = []
    if metrics.run_days < VALIDATION_DAYS_REQUIRED:
        reasons.append(
            f"run lasted {metrics.run_days} days; required {VALIDATION_DAYS_REQUIRED}"
        )
    if metrics.trades < thresholds.min_trades:
        reasons.append(
            f"{metrics.trades} trades observed; required at least {thresholds.min_trades}"
        )
    if metrics.max_drawdown_pct > thresholds.max_drawdown_pct:
        reasons.append(
            f"max drawdown {metrics.max_drawdown_pct}% exceeded {thresholds.max_drawdown_pct}%"
        )
    if metrics.slippage_bps_vs_backtest > thresholds.max_slippage_bps_vs_backtest:
        reasons.append(
            "slippage "
            f"{metrics.slippage_bps_vs_backtest} bps exceeded "
            f"{thresholds.max_slippage_bps_vs_backtest} bps"
        )
    if metrics.risk_guard_events > thresholds.max_risk_guard_events:
        reasons.append(
            f"{metrics.risk_guard_events} risk-guard events exceeded "
            f"{thresholds.max_risk_guard_events}"
        )
    if metrics.exec_errors > thresholds.max_exec_errors:
        reasons.append(
            f"{metrics.exec_errors} execution errors exceeded {thresholds.max_exec_errors}"
        )
    if thresholds.require_walk_forward and not metrics.walk_forward_passed:
        reasons.append("walk-forward check failed")
    if thresholds.require_oos and not metrics.oos_passed:
        reasons.append("out-of-sample check failed")
    return {"passed": not reasons, "reasons": reasons}


@router.post("", status_code=status.HTTP_201_CREATED)
async def provision_validation(
    request: Request,
    body: ProvisionValidationBody,
    user_id: str = Depends(_user_id),
) -> dict[str, Any]:
    sb = request.app.state.supabase
    settings = request.app.state.settings

    active = (
        sb.table("validation_runs")
        .select("id", count="exact")
        .eq("user_id", user_id)
        .in_("status", ["provisioning", "running"])
        .execute()
    )
    if (active.count or 0) >= settings.max_concurrent_validators:
        raise HTTPException(status_code=429, detail="Concurrent validator cap reached")

    spend = (
        sb.table("validation_runs")
        .select("estimated_cost_cents")
        .eq("user_id", user_id)
        .execute()
    )
    already_spent = sum(int(row.get("estimated_cost_cents") or 0) for row in (spend.data or []))
    projected_spend = already_spent + settings.validation_machine_estimated_cost_cents
    if projected_spend > settings.validation_monthly_budget_cents:
        raise HTTPException(status_code=402, detail="Monthly validation budget cap exceeded")

    region, fallbacks = EXCHANGE_REGIONS[body.exchange]
    expires_at = dt.datetime.now(dt.UTC) + dt.timedelta(hours=settings.validation_machine_ttl_hours)
    row = {
        "user_id": user_id,
        "strategy_id": body.strategy_id,
        "spec_hash": body.spec_hash,
        "mode": "paper",
        "exchange": body.exchange,
        "status": "provisioning",
        "idempotency_key": body.idempotency_key,
        "fly_app_name": settings.fly_app_name,
        "fly_region": region,
        "fly_fallback_regions": fallbacks,
        "lease_key": f"validation:{user_id}:{body.idempotency_key}",
        "ttl_expires_at": expires_at.isoformat(),
        "estimated_cost_cents": settings.validation_machine_estimated_cost_cents,
        "labels": {
            "user_id": user_id,
            "strategy_id": body.strategy_id,
            "mode": "paper",
            "ttl_hours": str(settings.validation_machine_ttl_hours),
        },
    }
    inserted = sb.table("validation_runs").upsert(
        row,
        on_conflict="user_id,idempotency_key",
    ).execute()
    validation = (inserted.data or [row])[0]
    return {
        "validation_id": validation.get("id"),
        "status": validation.get("status", "provisioning"),
        "machine": {
            "app": settings.fly_app_name,
            "region": region,
            "fallback_regions": fallbacks,
            "lease_key": row["lease_key"],
            "ttl_expires_at": row["ttl_expires_at"],
            "labels": row["labels"],
        },
    }


@router.post("/{validation_id}/scorecard")
async def grade_scorecard(
    validation_id: str,
    request: Request,
    body: ScorecardBody,
) -> dict[str, Any]:
    result = evaluate_scorecard(body.thresholds, body.metrics)
    status_value = "passed" if result["passed"] else "failed"
    request.app.state.supabase.table("validation_runs").update(
        {
            "scorecard_thresholds": body.thresholds.model_dump(),
            "scorecard_metrics": body.metrics.model_dump(),
            "scorecard_result": result,
            "status": status_value,
            "completed_at": dt.datetime.now(dt.UTC).isoformat(),
        }
    ).eq("id", validation_id).execute()
    return {"validation_id": validation_id, "status": status_value, **result}


@router.post("/janitor/destroy-list")
async def janitor_destroy_list(body: JanitorBody) -> dict[str, Any]:
    now = body.now or dt.datetime.now(dt.UTC)
    destroy = [
        machine.id
        for machine in body.machines
        if machine.expires_at <= now or machine.validation_id not in body.active_validation_ids
    ]
    return {"destroy_machine_ids": destroy}
