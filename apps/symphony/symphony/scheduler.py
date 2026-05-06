from __future__ import annotations

import asyncio
from collections import deque
from datetime import UTC, datetime, timedelta
from pathlib import Path

from symphony.agent import AsyncAgentRunner
from symphony.config import SymphonyConfig
from symphony.github import GitHubPublisher
from symphony.local_store import LocalStateStore
from symphony.tasks import AgentConfig, Task, TaskKind, TaskStatus
from symphony.workspace import WorkspaceManager


class RateLimiter:
    def __init__(self, *, max_per_hour: int, min_interval_seconds: int) -> None:
        self.max_per_hour = max_per_hour
        self.min_interval_seconds = min_interval_seconds
        self.launches: deque[datetime] = deque()

    def can_start(self) -> bool:
        now = datetime.now(UTC)
        while self.launches and self.launches[0] < now - timedelta(hours=1):
            self.launches.popleft()
        if len(self.launches) >= self.max_per_hour:
            return False
        if self.launches and (now - self.launches[-1]).total_seconds() < self.min_interval_seconds:
            return False
        return True

    def record_start(self) -> None:
        self.launches.append(datetime.now(UTC))


class SymphonyScheduler:
    def __init__(
        self,
        config: SymphonyConfig,
        store: LocalStateStore,
        *,
        publisher: GitHubPublisher | None = None,
    ) -> None:
        self.config = config
        self.store = store
        self.publisher = publisher or GitHubPublisher(remote=config.github_remote)
        self.rate_limiter = RateLimiter(
            max_per_hour=config.max_agent_starts_per_hour,
            min_interval_seconds=config.agent_start_interval_seconds,
        )
        self._running: set[str] = set()
        self._background_tasks: set[asyncio.Task[None]] = set()
        self._stop = asyncio.Event()

    async def run_forever(self) -> None:
        while not self._stop.is_set():
            await self.tick()
            try:
                await asyncio.wait_for(self._stop.wait(), timeout=self.config.poll_interval_seconds)
            except TimeoutError:
                pass

    def stop(self) -> None:
        self._stop.set()

    async def tick(self) -> None:
        await self._monitor_merged_tasks()
        if len(self._running) >= self.config.max_agents:
            return
        for task in self.store.list_tasks():
            if task.status not in {TaskStatus.CREATED, TaskStatus.DELEGATED, TaskStatus.CI_FAILED}:
                continue
            if task.id in self._running:
                continue
            if len(self._running) >= self.config.max_agents:
                break
            agent = self._select_agent(task)
            if agent is None or not self.rate_limiter.can_start():
                continue
            if self.store.count_running_for_agent(agent.id) >= agent.max_concurrent_tasks:
                continue
            self.rate_limiter.record_start()
            self._running.add(task.id)
            task.agent_id = agent.id
            task.status = TaskStatus.DELEGATED
            self.store.upsert_task(task)
            background_task = asyncio.create_task(self._run_task(task.id, agent))
            self._background_tasks.add(background_task)
            background_task.add_done_callback(self._background_tasks.discard)

    def _select_agent(self, task: Task) -> AgentConfig | None:
        if task.agent_id:
            agent = self.store.get_agent(task.agent_id)
            return agent if agent and agent.enabled else None
        agents = [
            agent
            for agent in self.store.list_agents()
            if agent.enabled and (task.kind == TaskKind.REVIEW or agent.role != "review")
        ]
        if task.kind == TaskKind.REVIEW:
            review_agents = [agent for agent in agents if agent.role == "review"]
            agents = review_agents or agents
        return agents[0] if agents else None

    async def _run_task(self, task_id: str, agent: AgentConfig) -> None:
        task = self.store.get_task(task_id)
        if task is None:
            self._running.discard(task_id)
            return
        try:
            task.status = (
                TaskStatus.RUNNING if task.kind != TaskKind.REVIEW else TaskStatus.REVIEWING
            )
            self.store.upsert_task(task)
            workspace = self._prepare_workspace(task)
            prompt = self._build_prompt(task)
            result = await AsyncAgentRunner(agent.command).run(workspace=workspace, prompt=prompt)
            if not result.ok:
                task.status = TaskStatus.FAILED
                task.error = result.stderr or result.stdout
                self.store.upsert_task(task)
                return
            if task.kind == TaskKind.REVIEW:
                await self._complete_review(task, result.stdout)
                return
            await self._publish_or_finish(task)
        except Exception as exc:
            task.status = TaskStatus.FAILED
            task.error = str(exc)
            self.store.upsert_task(task)
        finally:
            self._running.discard(task_id)

    def _prepare_workspace(self, task: Task) -> Path:
        repo_url = task.repo_url or self.config.default_repo_url
        if not repo_url:
            raise ValueError("task repoUrl or config defaultRepoUrl is required")
        workspace = WorkspaceManager(self.config.workspace_base).prepare(
            issue_identifier=task.id,
            repo_url=repo_url,
            base_branch=task.base_branch,
            branch_prefix=self.config.branch_prefix,
        )
        task.branch = workspace.branch
        task.workspace = str(workspace.path)
        self.store.upsert_task(task)
        return workspace.path

    async def _publish_or_finish(self, task: Task) -> None:
        if self.config.auto_publish_prs and task.workspace and task.branch:
            pr = await self.publisher.publish(
                workspace=Path(task.workspace),
                branch=task.branch,
                title=task.title,
                body=task.description,
            )
            task.pr_url = pr.url
            task.pr_number = pr.number
            task.status = TaskStatus.PR_CREATED
            self.store.upsert_task(task)
            review = Task(
                title=f"Review PR for {task.title}",
                description=f"Review implementation task {task.id}: {task.pr_url}",
                kind=TaskKind.REVIEW,
                parent_task_id=task.id,
                repo_url=task.repo_url,
                base_branch=task.base_branch,
                pr_url=task.pr_url,
                pr_number=task.pr_number,
            )
            self.store.upsert_task(review)
            return
        task.status = TaskStatus.FINISHED
        self.store.upsert_task(task)

    async def _complete_review(self, task: Task, review_output: str) -> None:
        task.review_notes = review_output
        has_issue = "NEEDS_FIX" in review_output or "REQUEST_CHANGES" in review_output
        parent = self.store.get_task(task.parent_task_id) if task.parent_task_id else None
        if has_issue and parent:
            fix = Task(
                title=f"Fix review findings for {parent.title}",
                description=f"Review agent found issues:\n\n{review_output}",
                kind=TaskKind.FIX,
                parent_task_id=parent.id,
                repo_url=parent.repo_url,
                base_branch=parent.branch or parent.base_branch,
                pr_url=parent.pr_url,
                pr_number=parent.pr_number,
            )
            task.status = TaskStatus.FINISHED
            parent.status = TaskStatus.FAILED
            parent.review_notes = review_output
            self.store.upsert_task(parent)
            self.store.upsert_task(task)
            self.store.upsert_task(fix)
            return
        task.status = TaskStatus.FINISHED
        self.store.upsert_task(task)
        if parent:
            parent.status = TaskStatus.WAITING_HUMAN_REVIEW
            parent.review_notes = review_output
            self.store.upsert_task(parent)

    def _build_prompt(self, task: Task) -> str:
        return f"""You are a local Symphony Codex agent.

Task: {task.title}
Kind: {task.kind}
Task ID: {task.id}
PR: {task.pr_url or "n/a"}

{task.description}

Workflow:
- Implement or review the task autonomously in this workspace.
- For review tasks, output APPROVED when acceptable.
- For review tasks with problems, output NEEDS_FIX and list every required fix.
- Keep changes scoped and run relevant tests before finishing.
"""

    async def _monitor_merged_tasks(self) -> None:
        for task in self.store.list_tasks():
            if task.status != TaskStatus.MERGED or not task.workspace or not task.pr_number:
                continue
            check_state = await self.publisher.checks_state(
                workspace=Path(task.workspace),
                pr_number=task.pr_number,
            )
            if check_state == "pending":
                continue
            if check_state == "passed":
                task.status = TaskStatus.FINISHED
                self.store.upsert_task(task)
                continue
            task.status = TaskStatus.CI_FAILED
            self.store.upsert_task(task)
            fix = Task(
                title=f"Fix CI failures for {task.title}",
                description=(
                    f"CI failed after merge for task {task.id}. "
                    "Inspect GitHub checks, fix the failure, and open a follow-up PR."
                ),
                kind=TaskKind.CI_FIX,
                parent_task_id=task.id,
                repo_url=task.repo_url,
                base_branch=task.base_branch,
                pr_url=task.pr_url,
                pr_number=task.pr_number,
            )
            self.store.upsert_task(fix)
