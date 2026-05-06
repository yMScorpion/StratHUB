from __future__ import annotations

import re
import subprocess
from dataclasses import dataclass
from pathlib import Path
from typing import Protocol


class CommandRunner(Protocol):
    def __call__(
        self,
        args: list[str],
        *,
        cwd: Path | None = None,
    ) -> subprocess.CompletedProcess[str]: ...


def default_command_runner(
    args: list[str], *, cwd: Path | None = None
) -> subprocess.CompletedProcess[str]:
    return subprocess.run(args, cwd=cwd, check=True, text=True, capture_output=True)


@dataclass(frozen=True)
class Workspace:
    path: Path
    branch: str


class WorkspaceManager:
    def __init__(
        self,
        base_dir: Path,
        *,
        command_runner: CommandRunner = default_command_runner,
    ) -> None:
        self.base_dir = base_dir
        self._run = command_runner

    def prepare(
        self,
        *,
        issue_identifier: str,
        repo_url: str,
        base_branch: str,
        branch_prefix: str,
    ) -> Workspace:
        self.base_dir.mkdir(parents=True, exist_ok=True)
        workspace_path = self.base_dir / sanitize_ref(issue_identifier)
        branch = f"{sanitize_ref(branch_prefix)}/{sanitize_ref(issue_identifier)}"

        if not (workspace_path / ".git").exists():
            self._run(["git", "clone", repo_url, str(workspace_path)])
        self._run(["git", "fetch", "origin", base_branch], cwd=workspace_path)
        self._run(["git", "checkout", "-B", branch, f"origin/{base_branch}"], cwd=workspace_path)
        return Workspace(path=workspace_path, branch=branch)


def sanitize_ref(value: str) -> str:
    slug = re.sub(r"[^A-Za-z0-9._-]+", "-", value.strip()).strip("-").lower()
    slug = re.sub(r"-{2,}", "-", slug)
    return slug or "issue"
