# Key Handling Threat Model

## Assets

- Broker API keys for Binance and Bybit.
- OpenRouter and OpenAI API keys, including BYOK user keys.
- KMS key IDs, encrypted DEKs, encrypted key ciphertext, nonce, scopes, and audit log entries.

## Trust Boundaries

- Browser and uploaded PDFs are untrusted.
- Next.js server can call `api-ai` with `X-Internal-Token`.
- `api-ai` can use the Supabase service role, but must not expose it downstream.
- Validator and executor machines receive only the minimum material required for a run.
- KMS plaintext DEKs and broker plaintext keys exist only in process memory for the shortest possible time.

## Threats And Controls

| Threat | Control | Evidence |
| --- | --- | --- |
| Browser exfiltrates service role or broker key | Service role stays server-side; `api_keys` stores only ciphertext; RLS denies cross-tenant reads | CI secret scan; RLS matrix |
| PDF prompt injection requests key disclosure | PDFs are untrusted data in the orchestrator prompt; generated output is Strategy Spec JSON only | `api_ai/ingest/orchestrator.py` |
| Leaked validator VM environment | Trade-only broker scopes; IP allowlist when exchange supports it; revoke key on incident | Key revocation runbook |
| Stale key remains usable after revocation | `revoked_at` checked before run launch; exchange key deleted or disabled; audit entry required | Key revocation runbook |
| Logs capture plaintext secrets | Structured logging allowlist; no request body logging for key routes; secret scan blocks committed values | CI secret scan |
| KMS/key wrapping misuse | Envelope encryption with `kms_key_id`, `dek_ciphertext`, `key_ciphertext`, and `nonce`; rotate DEK on key rotation | `api_keys` migration |
| LLM provider outage causes fallback to unsafe model | Strict JSON schema validation, semantic checks, citations, and no executable Python regardless of provider | Spec validation tests |

## Operational Requirements

- Broker keys must be trade-only; withdrawal permissions are disallowed.
- Live trading requires user confirmation and an active risk profile.
- Any key access must write an audit log entry with user, provider, key id, run id, and image digest.
- Revocation must stop new launches immediately and terminate active live validators within the runbook SLA.
