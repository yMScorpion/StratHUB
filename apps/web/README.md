# @cts/web

Next.js 15 (App Router) frontend for the crypto trading system.

## Scope by phase

- **Phase 0** (current): scaffolding, `/api/healthz`, placeholder pages.
- **Phase 2**: PDF upload UI with chunked/resumable uploads, signed URLs to Supabase Storage, job-status polling.
- **Phase 3**: Human review gate UI; Strategy Hub.
- **Phase 4**: Backtest results screen with TradingView Lightweight Charts.
- **Phase 5**: Validation dashboard.
- **Phase 6**: Live/paper account separation, kill-switch UI.
- **Phase 8**: Apple-tier polish.

## Conventions

- Route handlers stay thin: auth, signed URLs, enqueue jobs, status reads. No business logic.
- Heavy work goes to `apps/api-ai` (FastAPI) or `apps/exec-rs`.
- Live ↔ Paper toggle lives in a single Zustand slice.
