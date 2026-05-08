# Key Revocation Runbook

## Trigger

- User requests revocation.
- Broker key compromise is suspected.
- Internal service or validator machine exposure is suspected.

## Procedure

1. Set `api_keys.revoked_at = now()` for the affected key.
2. Disable or delete the key in the broker or LLM provider console.
3. Stop new launches that reference the key id.
4. Terminate active live runs using the key after cancel-all or flatten decisions are complete.
5. Rotate replacement key with trade-only scope and IP allowlist where supported.
6. Write audit log entries for revocation, provider-side action, and replacement.

## Recovery

- Require user confirmation before any strategy uses the replacement key.
- Verify no active process still has the old key id in memory or environment.
