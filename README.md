# crypto-trading-system

AI-driven crypto trading: PDF methodology → Strategy Spec → backtest → 7-day paper validation → live execution. Apple-tier UI, Supabase auth/data, Python/FastAPI for AI, single Rust binary for execution across backtest/paper/live.

> ⚠️ **Pre-release.** Phase 0 scaffold only. The system does not place real orders. The only thing it currently does is load and validate a Strategy Spec.

## Architecture in one paragraph

A user uploads PDFs (≤50/job). The Python AI pipeline OCRs them, embeds chunks into pgvector, calls **DeepSeek via OpenRouter with structured outputs (`json_schema strict`)**, and produces a constrained JSON **Strategy Spec** (a DSL — never executable Python). After a **mandatory human review gate**, the spec is passed by `spec_hash` to a single Rust binary (`apps/exec-rs`) that runs in one of three modes — `backtest`, `paper`, or `live` — sharing identical interpreter semantics. Validation runs spend 7 days on a Fly Machine pinned to the exchange region, executing virtually against live market data and graded against a configurable scorecard. Only after explicit user confirmation can the user promote a validated strategy to live capital with KMS-decrypted broker keys, trade-only scopes, and a hold-to-confirm kill-switch. Reproducibility is anchored in the canonical `spec_hash`, byte-identical across TypeScript, Python, and Rust.

## Repo layout

```
apps/
  web/              Next.js 15 (App Router, thin route handlers)
  api-ai/           FastAPI + Arq workers (PDF ingest, LLM, spec compile, jobs)
  exec-rs/          Single Rust binary — modes: backtest | paper | live
packages/
  strategy-spec/    JSON Schema (source of truth) + TS / Pydantic / serde bindings
infra/
  compose/          docker-compose.yml for local dev (Redis, api-ai, web)
  docker/           Dockerfiles per service
  supabase/         migrations, RLS policies
.github/workflows/  CI: rust, python, node, secret scan, migration review, hash parity
PLAN.md             Full architectural plan (v2 — incorporates 50-item review)
```

## Prereqs (macOS)

```sh
# Required
brew install node@22 rustup-init pnpm uv
rustup default 1.86.0
brew install --cask docker
brew install supabase/tap/supabase

# Or: install Node 22 + npm, then `npm install -g pnpm@9.15.0`
```

## Quickstart (Phase 0)

```sh
# 1. Install JS deps (root + workspaces)
pnpm install

# 2. Strategy-spec contract — verify all three languages agree on the canonical hash
cargo test -p strategy-spec --lib                    # 7 tests, hash parity asserted
( cd packages/strategy-spec/py && uv venv --python 3.12 .venv \
  && uv pip install --python .venv/bin/python -e . pytest \
  && .venv/bin/pytest -q )                           # 8 tests
( cd packages/strategy-spec/ts && node --test --import tsx src/index.test.ts )

# 3. Run the executor binary against the golden fixture
cargo run -p exec-rs -- --mode backtest \
  --spec packages/strategy-spec/fixtures/wyckoff_spring_btc_15m.json
# logs spec_hash=bfad590a... and exits (interpreter not yet implemented)

# 4. FastAPI service
( cd apps/api-ai && uv venv --python 3.12 .venv \
  && uv pip install --python .venv/bin/python -e . \
  && uv pip install --python .venv/bin/python --group dev pytest pytest-asyncio \
  && .venv/bin/pytest -q \
  && .venv/bin/uvicorn api_ai.main:app --reload --port 8000 )
# GET http://localhost:8000/healthz
# POST http://localhost:8000/spec/validate  (body: a Strategy Spec JSON)

# 5. Web app
( cd apps/web && pnpm dev )                          # http://localhost:3000

# 6. Local Supabase + full stack
supabase start                                       # boots :54321
docker compose -f infra/compose/docker-compose.yml up --build
```

## Verification

| What | Command | Expected |
|---|---|---|
| Rust strategy-spec | `cargo test -p strategy-spec --lib` | 7 passed |
| Rust workspace | `cargo test --workspace` | passes |
| Python strategy-spec | `pytest` in `packages/strategy-spec/py` | 8 passed |
| Python api-ai | `pytest` in `apps/api-ai` | 3 passed |
| TS strategy-spec | `node --test --import tsx src/index.test.ts` | 8 passed |
| Cross-language hash parity | all three suites pin `bfad590a34243fbb…` | identical |
| Web typecheck | `pnpm --filter @cts/web typecheck` | clean |

## Where things live

- **Architectural plan & roadmap** — `PLAN.md` (Phases 0 → 9, 14-week timeline for a 4-engineer team).
- **Strategy Spec contract & schema** — `packages/strategy-spec/` (the source of truth for what an "executable strategy" looks like).
- **Methodology source** — `Apostila Excelência Trading (4).pdf` (the user's PDF; VSA, Wyckoff, R:R ≥ 1:3, psychology). Phase 2 ingests this end-to-end.

## Status

| Phase | What | Status |
|---|---|---|
| 0 | Foundations & local dev (this commit) | **scaffolded, tested** |
| 1 | Strategy Spec contract | **shipped early** as part of Phase 0 |
| 2 | Real ingestion + DeepSeek (OpenRouter, structured outputs) | not started |
| 3 | Human review gate + Strategy Hub | not started |
| 4 | Backtest mode (deterministic, decimals, reproducibility) | not started |
| 5 | Paper mode + Fly validation cloud + scorecard grading | not started |
| 6 | Live mode + KMS decrypt + safeguards | not started |
| 7–9 | Hardening, polish, beta | not started |

## License

Proprietary. All rights reserved.
