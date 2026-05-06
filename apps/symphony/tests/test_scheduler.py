import asyncio
from pathlib import Path

from symphony.config import SymphonyConfig
from symphony.local_store import LocalStateStore
from symphony.scheduler import RateLimiter, SymphonyScheduler
from symphony.tasks import AgentConfig, Task, TaskStatus


class FakePublisher:
    async def checks_state(self, *, workspace: Path, pr_number: int) -> str:
        return "failed"


def make_config(tmp_path: Path) -> SymphonyConfig:
    return SymphonyConfig.model_validate(
        {
            "linearViewId": "view",
            "workflowFile": str(tmp_path / "workflow.md"),
            "workspaceBase": str(tmp_path / "workspaces"),
            "stateFile": str(tmp_path / "state.json"),
            "agentCommand": ["agent"],
            "defaultRepoUrl": "git@example.com:org/repo.git",
            "agentStartIntervalSeconds": 0,
            "maxAgentStartsPerHour": 1,
        }
    )


def test_rate_limiter_enforces_hourly_start_cap() -> None:
    limiter = RateLimiter(max_per_hour=1, min_interval_seconds=0)

    assert limiter.can_start()
    limiter.record_start()
    assert not limiter.can_start()


def test_scheduler_delegates_only_when_rate_limit_allows(tmp_path: Path) -> None:
    store = LocalStateStore(tmp_path / "local-state.json")
    store.upsert_agent(AgentConfig(name="Codex", command=["codex", "exec", "--stdin"]))
    first = store.upsert_task(Task(title="First", description="Do first"))
    second = store.upsert_task(Task(title="Second", description="Do second"))
    scheduler = SymphonyScheduler(make_config(tmp_path), store)

    assert scheduler.rate_limiter.can_start()
    scheduler.rate_limiter.record_start()

    asyncio.run(scheduler.tick())

    assert store.get_task(first.id).status == "created"
    assert store.get_task(second.id).status == "created"


def test_scheduler_creates_ci_fix_task_when_merged_pr_checks_fail(tmp_path: Path) -> None:
    store = LocalStateStore(tmp_path / "local-state.json")
    merged = store.upsert_task(
        Task(
            title="Merged feature",
            description="Watch CI",
            status=TaskStatus.MERGED,
            workspace=str(tmp_path),
            pr_number=123,
        )
    )
    scheduler = SymphonyScheduler(make_config(tmp_path), store, publisher=FakePublisher())

    asyncio.run(scheduler.tick())

    parent = store.get_task(merged.id)
    assert parent.status == TaskStatus.CI_FAILED
    fix_tasks = [task for task in store.list_tasks() if task.parent_task_id == merged.id]
    assert fix_tasks[0].kind == "ci_fix"
