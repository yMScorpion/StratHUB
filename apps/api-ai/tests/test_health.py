from __future__ import annotations

import json
from pathlib import Path

from fastapi.testclient import TestClient

from api_ai.main import create_app


FIXTURE = (
    Path(__file__).resolve().parents[3]
    / "packages"
    / "strategy-spec"
    / "fixtures"
    / "wyckoff_spring_btc_15m.json"
)


def test_healthz() -> None:
    client = TestClient(create_app())
    r = client.get("/healthz")
    assert r.status_code == 200
    body = r.json()
    assert body["status"] == "ok"
    assert body["service"] == "api-ai"


def test_spec_validate_golden() -> None:
    client = TestClient(create_app())
    spec = json.loads(FIXTURE.read_text("utf-8"))
    r = client.post("/spec/validate", json=spec)
    assert r.status_code == 200, r.text
    body = r.json()
    assert body["ok"] is True
    assert len(body["spec_hash"]) == 64
    assert body["sealed"]["spec_hash"] == body["spec_hash"]


def test_spec_validate_rejects_bad_per_trade_pct() -> None:
    client = TestClient(create_app())
    spec = json.loads(FIXTURE.read_text("utf-8"))
    spec["risk"]["per_trade_pct"] = 0.5  # number, not decimal-string
    r = client.post("/spec/validate", json=spec)
    assert r.status_code == 400
    assert r.json()["detail"]["kind"] == "schema"


def test_spec_validate_rejects_bogus_declared_hash() -> None:
    client = TestClient(create_app())
    spec = json.loads(FIXTURE.read_text("utf-8"))
    spec["spec_hash"] = "a" * 64  # valid hex format but wrong value
    r = client.post("/spec/validate", json=spec)
    assert r.status_code == 422
    assert r.json()["detail"]["kind"] == "hash_mismatch"


def test_spec_validate_rejects_invalid_uuid_pdf_id() -> None:
    client = TestClient(create_app())
    spec = json.loads(FIXTURE.read_text("utf-8"))
    spec["citations"][0]["pdf_id"] = "not-a-uuid"
    r = client.post("/spec/validate", json=spec)
    assert r.status_code == 400
    assert r.json()["detail"]["kind"] == "schema"
