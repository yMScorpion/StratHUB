from __future__ import annotations

from datetime import UTC, datetime
from enum import StrEnum
from pathlib import Path
from uuid import uuid4

from pydantic import BaseModel, Field


class TaskStatus(StrEnum):
    CREATED = "created"
    DELEGATED = "delegated"
    RUNNING = "running"
    PR_CREATED = "pr_created"
    REVIEWING = "reviewing"
    WAITING_HUMAN_REVIEW = "waiting_human_review"
    FINISHED = "finished"
    MERGED = "merged"
    CI_FAILED = "ci_failed"
    FAILED = "failed"


class TaskKind(StrEnum):
    IMPLEMENTATION = "implementation"
    REVIEW = "review"
    FIX = "fix"
    CI_FIX = "ci_fix"


class AgentConfig(BaseModel):
    id: str = Field(default_factory=lambda: f"agent-{uuid4().hex[:8]}")
    name: str
    command: list[str]
    role: str = "implementation"
    max_concurrent_tasks: int = 1
    enabled: bool = True


class Task(BaseModel):
    id: str = Field(default_factory=lambda: f"task-{uuid4().hex[:10]}")
    title: str
    description: str
    kind: TaskKind = TaskKind.IMPLEMENTATION
    status: TaskStatus = TaskStatus.CREATED
    agent_id: str | None = None
    parent_task_id: str | None = None
    repo_url: str | None = None
    base_branch: str = "main"
    branch: str | None = None
    workspace: str | None = None
    pr_url: str | None = None
    pr_number: int | None = None
    review_notes: str | None = None
    error: str | None = None
    created_at: datetime = Field(default_factory=lambda: datetime.now(UTC))
    updated_at: datetime = Field(default_factory=lambda: datetime.now(UTC))

    def touch(self) -> None:
        self.updated_at = datetime.now(UTC)


class CreateTaskRequest(BaseModel):
    title: str
    description: str
    agent_id: str | None = None
    repo_url: str | None = None
    base_branch: str = "main"


class CreateAgentRequest(BaseModel):
    name: str
    command: list[str]
    role: str = "implementation"
    max_concurrent_tasks: int = 1


class SymphonyLocalState(BaseModel):
    agents: dict[str, AgentConfig] = Field(default_factory=dict)
    tasks: dict[str, Task] = Field(default_factory=dict)
    launch_timestamps: list[datetime] = Field(default_factory=list)


def task_workspace(base: Path, task: Task) -> Path:
    return base / task.id
