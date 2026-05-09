# LLM Outage Runbook

## Trigger

- OpenRouter or configured model returns sustained 429, 5xx, timeout, or invalid structured output.
- LLM cost/user dashboard shows runaway spend.

## Procedure

1. Disable new ingestion submissions if budget or invalid-output alerts fire.
2. Confirm fallback model behavior still passes schema, semantic, citation, and hash checks.
3. Requeue failed jobs only after provider health recovers.
4. Preserve uploaded PDFs and extracted chunks; do not ask users to re-upload unless storage failed.
5. Record provider, model, request ids, affected jobs, token counts, and costs.

## Recovery

- Re-enable ingestion gradually.
- Audit failed outputs for prompt-injection regressions before bulk replay.
