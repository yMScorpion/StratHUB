#!/usr/bin/env python3
"""Verify Phase 7 hardening artifacts are present and internally consistent."""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]

REQUIRED_LOAD_OBJECTIVES = {
    "50-pdf-batch",
    "200-concurrent-backtests",
    "50-concurrent-validators",
    "pooler-holds",
}

REQUIRED_DASHBOARD_PANELS = {
    "Queue Depth",
    "LLM Cost/User",
    "Validator Fleet",
    "Fill Latency",
    "Clock Skew",
    "4xx/5xx By Exchange",
}

REQUIRED_RUNBOOKS = {
    "kill-switch.md",
    "broker-outage.md",
    "llm-outage.md",
    "region-failover.md",
    "key-revocation.md",
    "janitor-failure.md",
}

REQUIRED_RLS_OBJECTS = {
    "profiles",
    "api_keys",
    "risk_limits",
    "strategy_jobs",
    "pdf_uploads",
    "strategy_embeddings",
    "strategies",
    "backtests",
    "validation_runs",
    "accounts",
    "positions",
    "order_outbox",
    "trades",
    "fills",
    "audit_log",
    "heartbeats_validator",
}


def _load_json(path: str) -> object:
    with (ROOT / path).open("r", encoding="utf-8") as fh:
        return json.load(fh)


def _migration_tables() -> set[str]:
    tables: set[str] = set()
    for path in (ROOT / "infra/supabase/migrations").glob("*.sql"):
        text = path.read_text(encoding="utf-8")
        for match in re.finditer(r"create\s+table\s+if\s+not\s+exists\s+([a-z_]+)", text, re.I):
            name = match.group(1)
            if name not in {"spatial_ref_sys"}:
                tables.add(name)
    return tables


def verify() -> list[str]:
    errors: list[str] = []

    scenarios = _load_json("infra/hardening/load/scenarios.json")
    objective_names = {item["name"] for item in scenarios["objectives"]}  # type: ignore[index]
    missing_objectives = REQUIRED_LOAD_OBJECTIVES - objective_names
    if missing_objectives:
        errors.append(f"missing load objectives: {sorted(missing_objectives)}")

    matrix = _load_json("infra/hardening/security/rls-matrix.json")
    matrix_tables = {item["name"] for item in matrix["tables"]}  # type: ignore[index]
    missing_required_rls = REQUIRED_RLS_OBJECTS - matrix_tables
    if missing_required_rls:
        errors.append(f"RLS matrix missing required Phase 7 objects: {sorted(missing_required_rls)}")
    migration_tables = _migration_tables()
    missing_tables = migration_tables - matrix_tables
    if missing_tables:
        errors.append(f"RLS matrix missing migration tables: {sorted(missing_tables)}")
    for table in matrix["tables"]:  # type: ignore[index]
        for key in ("owner", "other_tenant"):
            ops = set(table[key])
            if ops != {"select", "insert", "update", "delete"}:
                errors.append(f"{table['name']} {key} ops are incomplete: {sorted(ops)}")

    dashboard = _load_json("infra/hardening/observability/grafana/phase7-dashboard.json")
    panel_titles = {panel["title"] for panel in dashboard["panels"]}  # type: ignore[index]
    missing_panels = REQUIRED_DASHBOARD_PANELS - panel_titles
    if missing_panels:
        errors.append(f"dashboard missing panels: {sorted(missing_panels)}")

    runbooks_dir = ROOT / "infra/hardening/runbooks"
    runbooks = {path.name for path in runbooks_dir.glob("*.md")}
    missing_runbooks = REQUIRED_RUNBOOKS - runbooks
    if missing_runbooks:
        errors.append(f"missing runbooks: {sorted(missing_runbooks)}")

    threat_model = ROOT / "infra/hardening/security/key-handling-threat-model.md"
    threat_text = threat_model.read_text(encoding="utf-8")
    for phrase in ("Broker API keys", "KMS", "revoked_at", "trade-only"):
        if phrase not in threat_text:
            errors.append(f"key handling threat model missing phrase: {phrase}")

    return errors


def main() -> int:
    errors = verify()
    if errors:
        for error in errors:
            print(f"ERROR: {error}", file=sys.stderr)
        return 1
    print("phase 7 hardening artifacts verified")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
