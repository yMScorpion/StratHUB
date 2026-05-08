# Janitor Failure Runbook

## Trigger

- TTL cleanup lag exceeds alert threshold.
- Orphaned validator machines, stale strategy jobs, or unreconciled outbox rows accumulate.

## Procedure

1. Disable destructive janitor actions until the failure mode is understood.
2. Snapshot candidate rows and machines before manual cleanup.
3. Re-run janitor in dry-run mode and compare planned deletes against retention policy.
4. Manually terminate orphaned machines only after confirming no active heartbeat.
5. Mark stale jobs failed with a user-visible error and preserve uploaded PDFs for retry.

## Recovery

- Re-enable janitor with a smaller batch size.
- Watch queue depth, validator fleet, and pooler dashboards for one full cleanup cycle.
