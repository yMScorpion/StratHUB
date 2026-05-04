# Architectural Plan — Crypto Trading Ecosystem

> "Apostila → Spec → Capital." Production SaaS that ingests trading PDFs, has DeepSeek (via OpenRouter) translate them into a structured **Strategy Spec** (a constrained DSL — never executable Python), backtests the spec, paper-validates it on live market data with virtual execution for 7 days against a configurable scorecard, and lets users explicitly approve a "Go Live" with strong safeguards.

This document is v2 of the plan, incorporating 50 review items focused on safety, correctness, and reproducibility.

---

## 1. System Architecture

```mermaid
flowchart LR
    subgraph CLIENT["Client (Next.js 15)"]
        UI[Dashboard · Hub · Backtest · Review · Validate]
    end

    subgraph EDGE["Edge / API (auth + thin)"]
        GW[Next.js Route Handlers + tRPC<br/>auth · signed URLs · enqueue · status]
        SB[(Supabase: Auth · Postgres · RLS · Storage · Realtime · pgvector)]
        POOL[(Supabase Pooler / pgbouncer)]
    end

    subgraph BRAIN["AI Pipeline (Python · FastAPI · Arq workers)"]
        ING[PDF Ingestor<br/>PyMuPDF + Tesseract + figure extract]
        VIS[Vision pass<br/>chart-heavy pages]
        CHK[Chunker + Embedder]
        ORC[OpenRouter Orchestrator<br/>structured outputs json_schema strict]
        SPEC[Spec Validator<br/>JSON Schema + citations check]
        REVIEW[needs_review queue<br/>human approval]
    end

    subgraph EXEC_BACKEND["Single Executor Binary (Rust) — modes: backtest · paper · live"]
        INTERP[Spec Interpreter<br/>same semantics in all modes]
        ADAPT[Exchange Adapters<br/>Binance · Bybit]
        RISK[Risk Guard + Kill-switch]
        IDEMP[Order Outbox<br/>deterministic client_order_id]
        CLOCK[NTP/chrony · skew monitor]
    end

    subgraph DATA["Market Data"]
        HIST[(Historical OHLCV + L2/trades<br/>Parquet · DuckDB · snapshot IDs)]
        LIVE[Live WS feeds<br/>used for paper validation]
    end

    subgraph CLOUD["Validation & Live Fleet"]
        FLY[Fly Machines API<br/>region-pinned · TTL labels]
        JANITOR[External janitor cron<br/>destroys expired/orphaned VMs]
        INGEST[Batched event-ingest API<br/>fronts Postgres + Realtime Broadcast]
    end

    UI --> GW
    GW <--> SB
    GW -- enqueue --> ING
    ING --> VIS --> CHK --> ORC --> SPEC --> REVIEW --> SB
    GW -- backtest job --> EXEC_BACKEND
    EXEC_BACKEND <-- HIST
    GW -- "Validate (post-approval)" --> FLY
    FLY --> EXEC_BACKEND
    EXEC_BACKEND <-- LIVE
    EXEC_BACKEND --> INGEST --> POOL --> SB
    GW -- "Go Live (post-validation + confirm)" --> FLY
    JANITOR --> FLY
```

**Data flow.**

1. User uploads PDFs (≤ 50/job, per-file + batch size caps, MIME sniff, SHA-256 dedupe, resumable). Files land in `pdfs/{user_id}/{upload_id}.pdf`. A `strategy_jobs` row enters `queued`.
2. Arq worker extracts text (PyMuPDF), runs OCR with `por`+`eng` (Tesseract, page timeout, confidence stored), extracts figures; chart-heavy pages are routed to a vision-capable model for VSA/Wyckoff annotations or marked `requires_human_review`.
3. Chunker + embedder writes to `strategy_embeddings` with explicit `embedding_model` and `embedding_dim`.
4. Orchestrator calls OpenRouter using **structured outputs** (`response_format.type=json_schema`, `strict: true`, `require_parameters: true`); default model `deepseek/deepseek-v4-pro`, bulk/retry pass on `deepseek/deepseek-v4-flash`. Failover to a known-compatible structured-output model if the route refuses. Source chunks are wrapped as untrusted data; every generated rule must include source page citations.
5. Spec validator: JSON-Schema validation, citation presence, semantic sanity (e.g. R:R ≥ 1, valid timeframe, defined risk model). Spec is hashed (`spec_hash`) and immutable. Status → `needs_review`.
6. **Human review gate.** UI shows extracted rules + citations + risk model + assumptions. User approves → `approved`. Only then can backtest/validate/live proceed.
7. Backtest, paper, and live all run the **same Rust binary** with different mode flags, consuming the same immutable spec — eliminating research/live drift.
8. Validation uses **live market data + virtual execution** (no exchange testnet for the strategy itself); testnet is reserved for adapter/order-API smoke tests. Fly Machine TTL-labeled, region-pinned (`nrt` Binance, `sin` Bybit), provisioning idempotent with leases, fallback regions, and a janitor cron.
9. Telemetry from VMs goes through a batched **event-ingest API** behind the Supabase pooler — VMs do not hold long-lived Postgres connections. Realtime Broadcast for transient UI updates; Postgres Changes only on durable trade/fill rows.

---

## 2. Tech Stack & Justification

| Layer | Choice | Why |
|---|---|---|
| Frontend | Next.js 15 (App Router) + React 19 + TS | Server Components; route handlers stay thin (auth/enqueue/status only). |
| UI | Tailwind v4 + shadcn/ui + Radix + Framer Motion | Apple-tier polish is **deferred** to Phase 8; v1 ships clean and accessible. |
| Charts | TradingView Lightweight Charts + visx | Entries/exits overlays + heatmaps. |
| State | TanStack Query + tRPC + Zustand | Type-safe; thin edge. |
| Auth/DB | **Supabase** (Postgres 16, Auth, RLS, Storage, Realtime, pgvector) | Multi-tenant via RLS; Realtime for UI. |
| Pooling | Supabase Pooler (transaction mode) | Required for the validator fleet — no direct VM connections. |
| AI service | Python 3.12 + FastAPI + Pydantic v2 + **Arq** (Redis) | Async-native job queue with retries + dead-letter. |
| LLM | OpenRouter, structured outputs, slugs `deepseek/deepseek-v4-pro` and `…-flash` | Configurable; failover model declared. |
| Embeddings | OpenAI `text-embedding-3-small` (1536) **or** Voyage `voyage-3` (1024) — picked at deploy, recorded per row. | Avoid hardcoding dim; per-row metadata; separate vector tables if dims diverge. |
| PDF/OCR | PyMuPDF + Tesseract (`por`,`eng`) + unstructured.io | Native + scanned + layout. |
| Vision | OpenRouter vision-capable model on chart pages | Recover Wyckoff/VSA chart annotations. |
| Backtest + Live | **One** event-driven engine in Rust over the spec interpreter, fed by historical OHLCV+trades for backtest and live WS for paper/live | One semantics path. |
| Market data store | DuckDB over Parquet, snapshot-versioned, with fees/funding/spreads/contract specs/lot sizes/min notional/leverage/liquidation rules/delistings | Reproducibility. |
| Decimals | `rust_decimal` (Rust) + `decimal.Decimal` (Py) end-to-end | Never floats for price/qty/notional. |
| Schema source of truth | **JSON Schema** in `packages/strategy-spec`; generators emit TS, Pydantic, and serde validators | One canonical schema. |
| Secrets | **KMS envelope encryption** (AWS KMS or GCP KMS) for broker keys; trade-only/no-withdraw scopes; rotation; audit log | Cloud executors run while the user is offline; session-derived keys are incompatible with that. |
| Validation cloud | Fly Machines (only). Provider interface present; Nomad/Hetzner not implemented until Fly is proven a blocker. | YAGNI. |
| Observability | OTel → Grafana Cloud; Sentry for FE/BE | One pipeline; alerts on 429/418/403 bans, clock skew, validator failures. |
| Local dev | `docker-compose.yml` orchestrating Supabase local, Redis, FastAPI, web, exec-rs, with health checks and documented ports | Single command bring-up. |
| CI/CD | GitHub Actions, **PRs only to main**, RLS test matrix, secret scan, migration review, preview envs | No auto-deploy to main. |
| IaC/Secrets | Terraform + Doppler | Reproducible. |

---

## 3. Strategy Spec (the executable contract)

A constrained DSL, JSON-only, versioned, hashed. No LLM-generated Python is ever executed.

The Rust interpreter is the only execution engine. The Python compiler converts the LLM JSON to this spec, runs JSON-Schema + citation + semantic checks, and persists `spec_jsonb` + `spec_hash`. Backtest/paper/live all consume the same `spec_hash`.

See `packages/strategy-spec/schema/strategy-spec.schema.json` for the source-of-truth schema and `packages/strategy-spec/fixtures/wyckoff_spring_btc_15m.json` for a hand-validated example sourced from the Apostila methodology.

---

## 4. Supabase Schema (high-level)

Conventions:
- Every tenant-owned row carries `user_id uuid not null references auth.users(id) on delete cascade` (denormalized onto child/event tables too).
- RLS on every table: `using (user_id = auth.uid())` plus admin role bypass; **integration test matrix** asserts no cross-tenant reads or writes.
- All tables `created_at`, `updated_at`; updates managed by triggers.

Phase 0 ships `profiles`, `api_keys` (KMS envelope), `risk_limits`. Subsequent migrations land per phase: ingestion (pdf_uploads, strategy_jobs, strategy_embeddings, strategies), backtests, validation_runs, accounts, positions, order_outbox, trades, fills, audit_log, heartbeats_*. See `infra/supabase/README.md` for the rollout plan.

---

## 5. Implementation Roadmap (safety-first order)

Timeline assumes 4 engineers (1 FE, 1 Python/AI, 1 Rust, 1 platform). Each phase ends in a deployed preview env, RLS tests green, secret scan clean, demo recorded.

**Phase 0 — Foundations & Local Dev (week 1) ✅ scaffolded**
- Monorepo + Turborepo + pnpm + uv + cargo workspace + `rust-toolchain.toml`.
- Terraform: Supabase project, Fly org, Cloudflare zone, KMS keys, Doppler. *(Terraform written in Phase 0.5; for now: Supabase CLI for local dev.)*
- `docker-compose.yml` for Supabase local, Redis, FastAPI, web, exec-rs with health checks.
- CI: lint/test all langs, hash parity, secret scan, migration review, **PRs required to main**.
- Auth: magic-link + Apple/Google; `profiles` row on signup; jurisdiction + ToS + risk-ack capture (table columns shipped, UI in Phase 0.5).
- Verify: `docker compose up` brings the whole stack; sign up, jurisdiction recorded, all health checks green.

**Phase 1 — Strategy Spec (shipped early, with Phase 0)**
- `packages/strategy-spec`: JSON Schema, generated TS/Pydantic/serde validators, golden fixtures.
- Spec hashing helper (`canonicalize → sha256`), pinned cross-language hash (`bfad590a…`).
- Verify: generators agree byte-identically on the fixture's hash.

**Phase 2 — Ingestion Pipeline + LLM (weeks 3–4)**
- Upload UI: chunked/resumable, per-file 25 MB, batch 50, page-count cap, MIME sniff, SHA-256 dedupe, cancel.
- Storage RLS policies + path scheme `pdfs/{user_id}/...`.
- Ingestor: PyMuPDF + Tesseract (`por`+`eng`, page timeouts, confidence per page, OCR errors persisted) + figure extraction; chart-heavy pages → vision pass or `requires_human_review`.
- Chunker + embedder (chosen model recorded per row).
- OpenRouter orchestrator: `response_format=json_schema strict`, default `deepseek/deepseek-v4-pro`, retry-with-repair (≤2) on `…-flash`, declared failover model if structured output is rejected. Source chunks wrapped as untrusted; rules require citations; reject outputs lacking citations.
- BYOK or platform-paid decision (config flag). Pre-flight credit check (BYOK) or budget check (platform-paid) before enqueue.
- Concrete limits enforced: PDFs/job=50, jobs/day, OCR pages/day, LLM tokens/month, concurrent validators/user.
- Verify: 5 distinct PDFs (incl. Apostila) produce valid, distinct, citation-bearing specs; prompt-injection probe PDFs fail closed.

**Phase 3 — Human Review Gate + Hub (week 5)**
- Review UI shows extracted rules with page citations and assumptions; approve/reject/request-changes.
- `strategies.status` transitions: `needs_review → approved` required before any execution.
- Strategy Hub: faceted filters, sort, side-sheet detail (clean v1, polish later).
- Verify: a strategy cannot be backtested/validated/lived without `approved`; rejection writes audit log.

**Phase 4 — Single Rust Executor: Backtest Mode (weeks 6–7)**
- `apps/exec-rs --mode=backtest`: spec interpreter, deterministic, decimals end-to-end.
- Market data store: DuckDB+Parquet with `data_snapshot_id`, fees, funding, spreads, contract specs, lot/min notional, leverage/margin, liquidation rules, delistings.
- Intrabar handling: configurable conservative assumption + optional 1m/tick path for trailing stops & tight R:R.
- KPI bundle (Sharpe, Sortino, Calmar, Max DD, Profit Factor, Win Rate, Avg R:R, Expectancy, CAGR, Time-in-Market) + equity curve + heatmap.
- `kpi_jsonb.meta` records snapshot id, fee/slippage models, lib versions, image digest, seeds.
- Backtest UI with Risk Model side-rail (fixed/percent/Kelly/vol-target/trailing modes) and Re-run.
- Verify: reproducibility — same spec_hash + same data_snapshot_id → bit-identical KPIs across two runs.

**Phase 5 — Single Rust Executor: Paper Mode + Validation Cloud (weeks 8–9)**
- Paper mode = live WS market data + virtual execution (not exchange testnet for the strategy). Adapter testnet only for adapter smoke tests.
- Risk Guard, kill-switch, deterministic `client_order_id`, order outbox, reconciliation loop.
- Per-exchange token-bucket rate limits, request-weight tracking, WS reconnect throttling, 429/418/403 alerts.
- NTP/chrony in image, clock-skew monitor.
- Fly Machines provisioning: idempotent, machine leases, region pinning + fallback, TTL labels, external janitor cron, hard per-user budget caps.
- Batched event-ingest API (FastAPI) fronts Postgres via the pooler; VMs never hold direct connections.
- Validation scorecard (configurable): min trades, max DD, slippage vs backtest, risk-guard events, exec errors, walk-forward, OOS — not "PnL > 0" alone.
- Verify: a 7-day run completes; janitor destroys an artificially-orphaned VM; pass/fail explained on the UI; no Postgres connection spike.

**Phase 6 — Single Rust Executor: Live Mode + Safeguards (weeks 10–11)**
- Same binary, `--mode=live`, KMS envelope decrypt of broker keys at process start; trade-only scopes enforced.
- Per-exchange cancel-all + optional flatten on kill-switch; safe behavior when DB/API is unreachable (local cache + fail-closed).
- User-directed execution confirmations (hold-to-confirm), risk acknowledgements, audit logs on every live action.
- Jurisdiction + ToS gate before "Go Live" is offered; counsel-reviewed disclosures.
- Verify: $50 sub-account on Binance executes a single round-trip; reconciliation matches broker statement; kill-switch flattens within SLA.

**Phase 7 — Hardening (week 12)**
- Load: 50-PDF batch; 200 concurrent backtests; 50 concurrent validators; pooler holds.
- Security review: RLS test matrix exhaustive; key handling threat model; dependency + container scan.
- Observability: dashboards for queue depth, LLM cost/user, validator fleet, fill latency, clock skew, 4xx/5xx by exchange.
- Runbooks: kill-switch, broker outage, LLM outage, region failover, key revocation, janitor failure.

**Phase 8 — UI Polish (week 13)**
- Apple-tier pass: motion, glassmorphism, empty states, skeletons, a11y (axe + keyboard), responsive QA. Held until correctness is proven.

**Phase 9 — Private Beta (week 14)**
- Admin allowlist + hard spend caps. Stripe billing/credits **wired before public signup**; if public signup is needed earlier, billing moves up.
- 25 invited users; feedback loop; status page live.

---

## 6. Cross-cutting Concerns

- **Security.** No LLM-generated Python is executed. KMS envelope encryption for broker keys; trade-only scopes; rotation; revocation; audit log. All tenant tables denormalize `user_id` and have RLS; storage objects gated by path prefix. Prompt injection: PDFs are untrusted data, system rules are non-overridable, citations mandatory. Live trading requires explicit hold-to-confirm + risk-ack.
- **Reproducibility.** `spec_hash` is the contract; `data_snapshot_id` + `image_digest` + `seeds` make backtests bit-reproducible; live trades reference the same `spec_hash`.
- **Concurrency.** Validator fleet talks to Postgres via the pooler through a batched ingest API. Heartbeats throttled and rolled up. Realtime Broadcast for transient UI streams.
- **Idempotency.** Deterministic `client_order_id` (uuidv5(spec_hash, intent_id)); unique DB constraints; outbox + reconciliation; job-level `idempotency_key`.
- **Cost.** Per-user monthly LLM budget; per-user concurrent validator cap; janitor enforces TTL even if app state is wrong; admin allowlist during beta.
- **Compliance.** Jurisdiction capture + gating; counsel-reviewed disclosures before live; "user-directed execution" framing; no performance guarantees; audit logs for every state change.
- **CI/CD.** PRs to main only; required checks: lint, unit, integration, RLS matrix, secret scan, migration review, preview deploy green; trunk-based with phase branches.
