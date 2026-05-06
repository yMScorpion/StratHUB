from __future__ import annotations

import json
import threading
from pathlib import Path

from symphony.tasks import AgentConfig, SymphonyLocalState, Task


class LocalStateStore:
    def __init__(self, path: Path) -> None:
        self.path = path
        self._lock = threading.RLock()
        self.state = self._load()

    def list_agents(self) -> list[AgentConfig]:
        with self._lock:
            return list(self.state.agents.values())

    def upsert_agent(self, agent: AgentConfig) -> AgentConfig:
        with self._lock:
            self.state.agents[agent.id] = agent
            self.save()
            return agent

    def get_agent(self, agent_id: str) -> AgentConfig | None:
        with self._lock:
            return self.state.agents.get(agent_id)

    def list_tasks(self) -> list[Task]:
        with self._lock:
            return sorted(self.state.tasks.values(), key=lambda task: task.created_at, reverse=True)

    def get_task(self, task_id: str) -> Task | None:
        with self._lock:
            return self.state.tasks.get(task_id)

    def upsert_task(self, task: Task) -> Task:
        with self._lock:
            task.touch()
            self.state.tasks[task.id] = task
            self.save()
            return task

    def count_running_for_agent(self, agent_id: str) -> int:
        with self._lock:
            return sum(
                1
                for task in self.state.tasks.values()
                if task.agent_id == agent_id
                and task.status in {"delegated", "running", "reviewing"}
            )

    def save(self) -> None:
        self.path.parent.mkdir(parents=True, exist_ok=True)
        self.path.write_text(self.state.model_dump_json(indent=2), encoding="utf-8")

    def _load(self) -> SymphonyLocalState:
        if not self.path.exists():
            return SymphonyLocalState()
        with self.path.open("r", encoding="utf-8") as handle:
            return SymphonyLocalState.model_validate(json.load(handle))
