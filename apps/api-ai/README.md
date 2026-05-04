# api-ai

AI pipeline service for the crypto trading system.

## Phase 0 routes

- `GET  /healthz` — service health
- `POST /spec/validate` — schema + semantic validation, returns `spec_hash` and a sealed copy

## Phase 2+ planned routes

- `POST /uploads` — signed-URL issuance for PDF uploads to Supabase Storage
- `POST /jobs/strategy` — enqueue an Arq job: OCR → vision → DeepSeek (structured outputs) → spec compile → semantic check → `needs_review`
- `GET  /jobs/{id}` — job status
- `POST /backtests` — enqueue a backtest job, executed by `apps/exec-rs --mode=backtest`
- `POST /validations` — provision a Fly Machine running `exec-rs --mode=paper` for 7 days

## Quickstart

```
uv venv --python 3.12 .venv
uv pip install --python .venv/bin/python -e ".[dev]"
.venv/bin/uvicorn api_ai.main:app --reload --port 8000
.venv/bin/arq api_ai.worker.WorkerSettings   # Phase 2+ — queue worker
```

`OPENROUTER_MODEL_PRIMARY` defaults to `deepseek/deepseek-v4-pro`. If/when the OpenRouter slug
changes (or that exact model isn't available on your account), set the env var; the
`OPENROUTER_MODEL_FALLBACK` is used when structured-output mode is rejected by the primary.
