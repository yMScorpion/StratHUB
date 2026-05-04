# Supabase

Schema, RLS policies, and seed data for the project. Local development uses the Supabase CLI;
production uses Supabase Cloud.

## Local

```
brew install supabase/tap/supabase     # if not already installed
supabase start                          # boots Postgres, Auth, Storage on :54321
supabase db reset                       # apply all migrations from a clean slate
```

## Migrations

Files in `migrations/` are timestamped (`YYYYMMDDHHMMSS_*.sql`) and applied in order. Each
phase of the build adds its own migration:

- Phase 0 — `20260504000000_init.sql` — `profiles`, `api_keys`, `risk_limits`, RLS template.
- Phase 1 — strategy spec storage (column types only; the spec itself lives as JSONB).
- Phase 2 — ingestion pipeline tables: `pdf_uploads`, `strategy_jobs`, `strategy_job_uploads`,
  `strategy_job_events`, `strategy_embeddings`, `strategies`.
- Phase 3 — review state machine.
- Phase 4 — `backtests`.
- Phase 5 — `validation_runs`, telemetry tables, batched ingest API.
- Phase 6 — `accounts`, `positions`, `order_outbox`, `trades`, `fills`, `audit_log`.

## RLS test matrix

`tests/rls/` (added in Phase 0.5) contains a Vitest + supabase-js suite that authenticates as
two distinct users and asserts that neither can read, insert, update, or delete the other's
rows. CI fails if any cross-tenant operation succeeds.
