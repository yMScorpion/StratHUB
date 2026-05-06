from __future__ import annotations

import json
import os
from pathlib import Path
from typing import Any

from pydantic import BaseModel, Field, field_validator


class SymphonyConfig(BaseModel):
    """Runtime configuration loaded from symphony.json."""

    linear_api_key: str = Field(default="", validation_alias="linearApiKey")
    linear_view_id: str = Field(default="", validation_alias="linearViewId")
    workflow_file: Path = Field(default=Path("workflow.md"), validation_alias="workflowFile")
    workspace_base: Path = Field(
        default=Path(".symphony/workspaces"),
        validation_alias="workspaceBase",
    )
    state_file: Path | None = Field(default=None, validation_alias="stateFile")
    agent_command: list[str] = Field(
        default_factory=lambda: ["codex", "exec", "--stdin"],
        validation_alias="agentCommand",
    )
    default_agent_id: str = Field(default="codex", validation_alias="defaultAgentId")
    default_repo_url: str | None = Field(default=None, validation_alias="defaultRepoUrl")
    poll_interval_seconds: int = Field(default=60, ge=1, validation_alias="pollIntervalSeconds")
    max_concurrency: int = Field(default=1, ge=1, validation_alias="maxConcurrency")
    max_agents: int = Field(default=2, ge=1, validation_alias="maxAgents")
    max_agent_starts_per_hour: int = Field(
        default=12,
        ge=1,
        validation_alias="maxAgentStartsPerHour",
    )
    agent_start_interval_seconds: int = Field(
        default=300,
        ge=0,
        validation_alias="agentStartIntervalSeconds",
    )
    branch_prefix: str = Field(default="symphony", validation_alias="branchPrefix")
    max_retries: int = Field(default=1, ge=0, validation_alias="maxRetries")
    dry_run: bool = Field(default=False, validation_alias="dryRun")
    completed_state_id: str | None = Field(default=None, validation_alias="completedStateId")
    failure_state_id: str | None = Field(default=None, validation_alias="failureStateId")
    host: str = Field(default="127.0.0.1")
    port: int = Field(default=8765, ge=1, le=65535)
    auto_publish_prs: bool = Field(default=True, validation_alias="autoPublishPrs")
    github_remote: str = Field(default="origin", validation_alias="githubRemote")

    @field_validator("agent_command")
    @classmethod
    def agent_command_must_not_be_empty(cls, value: list[str]) -> list[str]:
        if not value:
            raise ValueError("agentCommand must contain at least one command token")
        return value

    @field_validator("linear_api_key", mode="before")
    @classmethod
    def expand_linear_api_key(cls, value: Any) -> Any:
        if isinstance(value, str) and value.startswith("env:"):
            return os.environ.get(value.removeprefix("env:"), "")
        return value

    @property
    def resolved_state_file(self) -> Path:
        return self.state_file or self.workspace_base / "symphony-state.json"


def load_config(path: str | Path) -> SymphonyConfig:
    config_path = Path(path)
    with config_path.open("r", encoding="utf-8") as handle:
        raw = json.load(handle)
    config = SymphonyConfig.model_validate(raw)
    if not config.workflow_file.is_absolute():
        config.workflow_file = config_path.parent / config.workflow_file
    if not config.workspace_base.is_absolute():
        config.workspace_base = config_path.parent / config.workspace_base
    if config.state_file is not None and not config.state_file.is_absolute():
        config.state_file = config_path.parent / config.state_file
    return config
