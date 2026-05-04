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

    @classmethod
    def redis_settings(cls) -> RedisSettings:
        url = load_settings().redis_url
        return RedisSettings.from_dsn(url)
