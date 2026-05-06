from __future__ import annotations

import asyncio
from collections.abc import AsyncIterator
from contextlib import asynccontextmanager
from pathlib import Path

from fastapi import FastAPI, HTTPException
from fastapi.responses import FileResponse
from fastapi.staticfiles import StaticFiles

from symphony.config import SymphonyConfig, load_config
from symphony.local_store import LocalStateStore
from symphony.scheduler import SymphonyScheduler
from symphony.tasks import AgentConfig, CreateAgentRequest, CreateTaskRequest, Task, TaskStatus


def create_app(config: SymphonyConfig) -> FastAPI:
    store = LocalStateStore(config.resolved_state_file)
    scheduler = SymphonyScheduler(config, store)
    static_dir = Path(__file__).parent / "static"

    @asynccontextmanager
    async def lifespan(app: FastAPI) -> AsyncIterator[None]:
        task = asyncio.create_task(scheduler.run_forever())
        yield
        scheduler.stop()
        task.cancel()

    app = FastAPI(title="Symphony", lifespan=lifespan)
    app.mount("/static", StaticFiles(directory=static_dir), name="static")

    @app.get("/")
    def index() -> FileResponse:
        return FileResponse(static_dir / "index.html")

    @app.get("/healthz")
    def healthz() -> dict[str, str]:
        return {"status": "ok"}

    @app.get("/api/agents")
    def list_agents() -> list[AgentConfig]:
        return store.list_agents()

    @app.post("/api/agents")
    def create_agent(request: CreateAgentRequest) -> AgentConfig:
        agent = AgentConfig(
            name=request.name,
            command=request.command,
            role=request.role,
            max_concurrent_tasks=request.max_concurrent_tasks,
        )
        return store.upsert_agent(agent)

    @app.get("/api/tasks")
    def list_tasks() -> list[Task]:
        return store.list_tasks()

    @app.post("/api/tasks")
    def create_task(request: CreateTaskRequest) -> Task:
        task = Task(
            title=request.title,
            description=request.description,
            agent_id=request.agent_id,
            repo_url=request.repo_url,
            base_branch=request.base_branch,
        )
        return store.upsert_task(task)

    @app.post("/api/tasks/{task_id}/assign/{agent_id}")
    def assign_task(task_id: str, agent_id: str) -> Task:
        task = store.get_task(task_id)
        if task is None:
            raise HTTPException(status_code=404, detail="task not found")
        if store.get_agent(agent_id) is None:
            raise HTTPException(status_code=404, detail="agent not found")
        task.agent_id = agent_id
        return store.upsert_task(task)

    @app.post("/api/tasks/{task_id}/merged")
    def mark_merged(task_id: str) -> Task:
        task = store.get_task(task_id)
        if task is None:
            raise HTTPException(status_code=404, detail="task not found")
        task.status = TaskStatus.MERGED
        return store.upsert_task(task)

    @app.post("/api/scheduler/tick")
    async def tick() -> dict[str, str]:
        await scheduler.tick()
        return {"status": "ok"}

    return app


def app_from_config_path(config_path: str) -> FastAPI:
    return create_app(load_config(config_path))
