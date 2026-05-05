"""FastAPI application entrypoint."""

from __future__ import annotations

from contextlib import asynccontextmanager
from typing import Any

from fastapi import FastAPI, HTTPException, status
from pydantic import BaseModel

from strategy_spec import (
    SchemaValidationError,
    hash_spec,
    semantic_check,
    validate,
    with_hash,
)

from .routes.jobs import router as jobs_router
from .settings import Settings, load_settings


def create_app() -> FastAPI:
    settings = load_settings()

    @asynccontextmanager
    async def lifespan(app: FastAPI):  # type: ignore[type-arg]
        from arq import create_pool
        from arq.connections import RedisSettings
        from supabase import create_client

        app.state.supabase = create_client(
            settings.supabase_url, settings.supabase_service_role_key
        )
        app.state.arq_redis = await create_pool(RedisSettings.from_dsn(settings.redis_url))
        yield
        await app.state.arq_redis.close()

    app = FastAPI(
        title="api-ai",
        version="0.2.0",
        description="AI pipeline: PDF → spec → backtest jobs.",
        lifespan=lifespan,
    )
    app.state.settings = settings
    app.include_router(jobs_router)

    @app.get("/healthz")
    async def healthz() -> dict[str, Any]:
        return {
            "status": "ok",
            "service": "api-ai",
            "phase": 2,
            "config": {
                "max_pdfs_per_job": settings.max_pdfs_per_job,
                "max_pdf_bytes": settings.max_pdf_bytes,
                "max_jobs_per_day": settings.max_jobs_per_day,
                "openrouter_model_primary": settings.openrouter_model_primary,
                "openrouter_model_fallback": settings.openrouter_model_fallback,
                "embedding_model": settings.embedding_model,
                "byok_mode": settings.byok_mode,
            },
        }

    @app.post("/spec/validate")
    async def spec_validate(spec: dict[str, Any]) -> dict[str, Any]:
        try:
            validate(spec)
        except SchemaValidationError as e:
            raise HTTPException(
                status_code=status.HTTP_400_BAD_REQUEST,
                detail={"kind": "schema", "errors": [str(err) for err in e.errors[:20]]},
            ) from e
        problems = semantic_check(spec)
        if problems:
            raise HTTPException(
                status_code=status.HTTP_400_BAD_REQUEST,
                detail={
                    "kind": "semantic",
                    "errors": [
                        {"code": p.code, "message": p.message, "path": p.path}
                        for p in problems
                    ],
                },
            )
        computed = hash_spec(spec)
        declared = spec.get("spec_hash")
        if declared is not None and declared != computed:
            raise HTTPException(
                status_code=422,
                detail={
                    "kind": "hash_mismatch",
                    "errors": [
                        f"declared spec_hash {declared!r} does not match computed {computed!r}"
                    ],
                },
            )
        return {"ok": True, "spec_hash": computed, "sealed": with_hash(spec)}

    return app


app = create_app()
