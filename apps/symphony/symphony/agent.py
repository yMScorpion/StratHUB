from __future__ import annotations

import asyncio
import subprocess
from dataclasses import dataclass
from pathlib import Path


@dataclass(frozen=True)
class AgentResult:
    exit_code: int
    stdout: str
    stderr: str

    @property
    def ok(self) -> bool:
        return self.exit_code == 0


class AgentRunner:
    def __init__(self, command: list[str], *, timeout_seconds: int | None = None) -> None:
        self.command = command
        self.timeout_seconds = timeout_seconds

    def run(self, *, workspace: Path, prompt: str) -> AgentResult:
        result = subprocess.run(
            self.command,
            cwd=workspace,
            input=prompt,
            text=True,
            capture_output=True,
            timeout=self.timeout_seconds,
            check=False,
        )
        return AgentResult(exit_code=result.returncode, stdout=result.stdout, stderr=result.stderr)


class AsyncAgentRunner:
    def __init__(self, command: list[str], *, timeout_seconds: int | None = None) -> None:
        self.command = command
        self.timeout_seconds = timeout_seconds

    async def run(self, *, workspace: Path, prompt: str) -> AgentResult:
        process = await asyncio.create_subprocess_exec(
            *self.command,
            cwd=workspace,
            stdin=asyncio.subprocess.PIPE,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
        )
        try:
            stdout, stderr = await asyncio.wait_for(
                process.communicate(prompt.encode()),
                timeout=self.timeout_seconds,
            )
        except TimeoutError:
            process.kill()
            stdout, stderr = await process.communicate()
            return AgentResult(
                exit_code=124,
                stdout=stdout.decode(errors="replace"),
                stderr=(stderr.decode(errors="replace") + "\nagent timed out").strip(),
            )
        return AgentResult(
            exit_code=process.returncode or 0,
            stdout=stdout.decode(errors="replace"),
            stderr=stderr.decode(errors="replace"),
        )
