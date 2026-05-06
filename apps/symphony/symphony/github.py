from __future__ import annotations

import asyncio
import json
import re
from dataclasses import dataclass
from pathlib import Path


@dataclass(frozen=True)
class PullRequest:
    url: str
    number: int | None = None


class GitHubPublisher:
    def __init__(self, *, remote: str = "origin") -> None:
        self.remote = remote

    async def publish(self, *, workspace: Path, branch: str, title: str, body: str) -> PullRequest:
        await self._run(["git", "add", "-A"], cwd=workspace)
        if await self._has_changes(workspace):
            await self._run(["git", "commit", "-m", title], cwd=workspace)
        await self._run(["git", "push", "-u", self.remote, branch], cwd=workspace)
        result = await self._run(
            ["gh", "pr", "create", "--title", title, "--body", body],
            cwd=workspace,
            check=False,
        )
        output = (result[0] + result[1]).strip()
        if "already exists" in output.lower():
            result = await self._run(["gh", "pr", "view", "--json", "url,number"], cwd=workspace)
            output = result[0].strip()
        return PullRequest(url=_extract_url(output), number=_extract_number(output))

    async def checks_passed(self, *, workspace: Path, pr_number: int) -> bool:
        return await self.checks_state(workspace=workspace, pr_number=pr_number) == "passed"

    async def checks_state(self, *, workspace: Path, pr_number: int) -> str:
        stdout, _ = await self._run(
            ["gh", "pr", "checks", str(pr_number), "--watch=false"],
            cwd=workspace,
            check=False,
        )
        lowered = stdout.lower()
        if "fail" in lowered or "error" in lowered or "cancel" in lowered:
            return "failed"
        if "pending" in lowered or "waiting" in lowered:
            return "pending"
        return "passed"

    async def _has_changes(self, workspace: Path) -> bool:
        stdout, _ = await self._run(["git", "status", "--porcelain"], cwd=workspace)
        return bool(stdout.strip())

    async def _run(
        self,
        args: list[str],
        *,
        cwd: Path,
        check: bool = True,
    ) -> tuple[str, str]:
        process = await asyncio.create_subprocess_exec(
            *args,
            cwd=cwd,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
        )
        stdout, stderr = await process.communicate()
        out = stdout.decode(errors="replace")
        err = stderr.decode(errors="replace")
        if check and process.returncode:
            raise RuntimeError(err or out or f"{args[0]} failed with {process.returncode}")
        return out, err


def _extract_url(output: str) -> str:
    try:
        parsed = json.loads(output)
        if isinstance(parsed, dict) and isinstance(parsed.get("url"), str):
            return parsed["url"]
    except json.JSONDecodeError:
        pass
    match = re.search(r"https://github\.com/\S+", output)
    return match.group(0) if match else output


def _extract_number(output: str) -> int | None:
    try:
        parsed = json.loads(output)
        if isinstance(parsed, dict) and isinstance(parsed.get("number"), int):
            return parsed["number"]
    except json.JSONDecodeError:
        pass
    match = re.search(r"/pull/(\d+)", output)
    if match:
        return int(match.group(1))
    match = re.search(r'"number"\s*:\s*(\d+)', output)
    return int(match.group(1)) if match else None
