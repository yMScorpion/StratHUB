"""Arq worker entrypoint. Phase 0: only the queue is wired. Real jobs land in Phase 2+."""

from __future__ import annotations

from arq.connections import RedisSettings

from .settings import load_settings


async def noop(_ctx: dict[str, object]) -> str:
    return "ok"


class WorkerSettings:
    """`arq apps.api-ai.api_ai.worker.WorkerSettings` to run."""

    functions = [noop]
    keep_result = 60
    # Must be a class-level attribute (not a classmethod) — arq reads it via class.__dict__.
    redis_settings = RedisSettings.from_dsn(load_settings().redis_url)
