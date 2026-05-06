from __future__ import annotations

import json
from pathlib import Path
from typing import Literal

from pydantic import BaseModel, Field

RunStatus = Literal["running", "completed", "failed"]


class RunRecord(BaseModel):
    issue_id: str
    issue_identifier: str
    branch: str
    workspace: str
    attempts: int = 0
    status: RunStatus = "running"
    last_error: str | None = None


class SymphonyState(BaseModel):
    runs: dict[str, RunRecord] = Field(default_factory=dict)


class StateStore:
    def __init__(self, path: Path) -> None:
        self.path = path
        self.state = self._load()

    def get(self, issue_id: str) -> RunRecord | None:
        return self.state.runs.get(issue_id)

    def upsert(self, record: RunRecord) -> None:
        self.state.runs[record.issue_id] = record
        self.save()

    def active_count(self) -> int:
        return sum(1 for run in self.state.runs.values() if run.status == "running")

    def save(self) -> None:
        self.path.parent.mkdir(parents=True, exist_ok=True)
        self.path.write_text(self.state.model_dump_json(indent=2), encoding="utf-8")

    def _load(self) -> SymphonyState:
        if not self.path.exists():
            return SymphonyState()
        with self.path.open("r", encoding="utf-8") as handle:
            return SymphonyState.model_validate(json.load(handle))
