"""FastAPI application entrypoint.

Phase 0 scope: health check + spec validate + spec hash. Phases 2+ add ingestion, OpenRouter
orchestration, spec compilation, and backtest enqueue routes.
"""

from __future__ import annotations

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

from .settings import load_settings


def create_app() -> FastAPI:
    settings = load_settings()
    app = FastAPI(
        title="api-ai",
        version="0.1.0",
        description="AI pipeline: PDF -> spec -> backtest jobs.",
    )
    app.state.settings = settings

    @app.get("/healthz")
    async def healthz() -> dict[str, Any]:
        return {
            "status": "ok",
            "service": "api-ai",
            "phase": 0,
            "config": {
                "max_pdfs_per_job": settings.max_pdfs_per_job,
                "openrouter_model_primary": settings.openrouter_model_primary,
                "openrouter_model_fallback": settings.openrouter_model_fallback,
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
        return {"ok": True, "spec_hash": hash_spec(spec), "sealed": with_hash(spec)}

    return app


app = create_app()
