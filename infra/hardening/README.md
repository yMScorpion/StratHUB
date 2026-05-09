# Phase 7 Hardening

This directory is the release gate for Phase 7.

It contains:

- `load/scenarios.json` - required load envelopes for ingestion, backtests, validators, and pooler holds.
- `security/rls-matrix.json` - tenant isolation matrix covering every current tenant table and PDF storage.
- `security/key-handling-threat-model.md` - broker and LLM key handling threat model.
- `observability/grafana/phase7-dashboard.json` - Grafana dashboard definition for the required production views.
- `runbooks/` - incident procedures for kill-switch, broker outage, LLM outage, region failover, key revocation, and janitor failure.

Run the local gate:

```sh
python3 tools/phase7/verify_hardening.py
```

CI also runs Trivy filesystem/config scans and builds/scans the runtime container images.
