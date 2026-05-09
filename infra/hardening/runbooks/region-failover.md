# Region Failover Runbook

## Trigger

- Primary Fly region cannot launch or maintain validator machines.
- Exchange region latency breaches fill latency SLO.
- Supabase or event-ingest path is unavailable in the active region.

## Procedure

1. Freeze live promotions in the affected region.
2. Route new validators to the approved secondary exchange-near region.
3. Keep existing live runs fail-closed unless market risk requires cancel/flatten.
4. Verify event-ingest writes through the pooler from the secondary region.
5. Watch clock skew and fill latency dashboards for the secondary fleet.

## Recovery

- Move back only after the primary region passes launch, heartbeat, event-ingest, and exchange latency probes.
- Keep a written timeline of run ids moved and run ids terminated.
