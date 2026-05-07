"""Batched validator event ingest.

Validation VMs call this API with batches of telemetry. The API writes durable rows through the
application's Supabase client/pooler boundary; validators never receive direct Postgres DSNs.
"""

from __future__ import annotations

import datetime as dt
from typing import Any, Literal

from fastapi import APIRouter, HTTPException, Request, status
from pydantic import BaseModel, Field

router = APIRouter(prefix="/events", tags=["events"])


class ValidationEvent(BaseModel):
    seq: int = Field(ge=0)
    kind: Literal[
        "heartbeat",
        "market_tick",
        "order_intent",
        "fill",
        "risk_guard",
        "exec_error",
        "rate_limit",
        "clock_skew",
        "scorecard",
    ]
    ts: dt.datetime
    payload: dict[str, Any] = Field(default_factory=dict)


class EventBatch(BaseModel):
    validation_id: str = Field(min_length=1, max_length=128)
    run_id: str = Field(min_length=1, max_length=128)
    events: list[ValidationEvent] = Field(min_length=1)


@router.post("/ingest", status_code=status.HTTP_202_ACCEPTED)
async def ingest_events(request: Request, batch: EventBatch) -> dict[str, Any]:
    settings = request.app.state.settings
    if len(batch.events) > settings.max_validation_event_batch:
        raise HTTPException(
            status_code=413,
            detail=f"event batch exceeds {settings.max_validation_event_batch} events",
        )

    rows = [
        {
            "validation_id": batch.validation_id,
            "run_id": batch.run_id,
            "seq": event.seq,
            "kind": event.kind,
            "event_ts": event.ts.isoformat(),
            "payload": event.payload,
        }
        for event in batch.events
    ]

    sb = request.app.state.supabase
    sb.table("validation_events").insert(rows).execute()

    last_event = max(batch.events, key=lambda event: event.seq)
    sb.table("validation_heartbeats").upsert(
        {
            "validation_id": batch.validation_id,
            "run_id": batch.run_id,
            "last_seq": last_event.seq,
            "last_event_at": last_event.ts.isoformat(),
            "updated_at": dt.datetime.now(dt.UTC).isoformat(),
        },
        on_conflict="validation_id",
    ).execute()

    return {"accepted": len(rows), "last_seq": last_event.seq}
