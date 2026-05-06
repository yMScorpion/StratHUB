from pathlib import Path

from symphony.agent import AgentResult
from symphony.config import SymphonyConfig
from symphony.linear import LinearIssue
from symphony.orchestrator import SymphonyOrchestrator
from symphony.state import RunRecord, StateStore
from symphony.workflow import WorkflowDefinition
from symphony.workspace import Workspace


class FakeTracker:
    def __init__(self, issues: list[LinearIssue]) -> None:
        self.issues = issues
        self.comments: list[tuple[str, str]] = []
        self.state_updates: list[tuple[str, str]] = []

    def list_view_issues(self, view_id: str) -> list[LinearIssue]:
        return self.issues

    def comment_issue(self, issue_id: str, body: str) -> None:
        self.comments.append((issue_id, body))

    def set_issue_state(self, issue_id: str, state_id: str) -> None:
        self.state_updates.append((issue_id, state_id))


class FakeWorkspaceManager:
    def __init__(self, path: Path) -> None:
        self.path = path
        self.calls: list[dict[str, str]] = []

    def prepare(
        self,
        *,
        issue_identifier: str,
        repo_url: str,
        base_branch: str,
        branch_prefix: str,
    ) -> Workspace:
        self.calls.append(
            {
                "issue_identifier": issue_identifier,
                "repo_url": repo_url,
                "base_branch": base_branch,
                "branch_prefix": branch_prefix,
            }
        )
        self.path.mkdir(parents=True, exist_ok=True)
        return Workspace(path=self.path, branch=f"{branch_prefix}/{issue_identifier.lower()}")


class FakeAgentRunner:
    def __init__(self, result: AgentResult | None = None) -> None:
        self.result = result or AgentResult(exit_code=0, stdout="ok", stderr="")
        self.prompts: list[str] = []

    def run(self, *, workspace: Path, prompt: str) -> AgentResult:
        self.prompts.append(prompt)
        return self.result


def make_config(tmp_path: Path) -> SymphonyConfig:
    return SymphonyConfig.model_validate(
        {
            "linearViewId": "view",
            "workflowFile": str(tmp_path / "workflow.md"),
            "workspaceBase": str(tmp_path / "workspaces"),
            "stateFile": str(tmp_path / "state.json"),
            "agentCommand": ["agent"],
            "defaultRepoUrl": "git@example.com:org/repo.git",
            "maxRetries": 1,
        }
    )


def test_poll_starts_runnable_issue_and_records_completion(tmp_path: Path) -> None:
    issue = LinearIssue(
        id="issue-id",
        identifier="ENG-1",
        title="Add service",
        description="Do the thing",
        state_name="Ready",
        url="https://linear.app/acme/issue/ENG-1",
    )
    workflow = WorkflowDefinition(
        repoUrl="git@example.com:org/repo.git",
        branchPrefix="bot",
        issue="Build",
        plan="Implement and test.",
        acceptance_criteria=["CI passes"],
        runnableStateNames=["Ready"],
    )
    tracker = FakeTracker([issue])
    agent = FakeAgentRunner()
    workspace = FakeWorkspaceManager(tmp_path / "workspace")
    state = StateStore(tmp_path / "state.json")

    summary = SymphonyOrchestrator(
        make_config(tmp_path),
        tracker=tracker,
        workflow=workflow,
        workspace_manager=workspace,
        agent_runner=agent,
        state_store=state,
    ).poll_once()

    assert summary.started == 1
    assert state.get("issue-id").status == "completed"
    assert workspace.calls[0]["branch_prefix"] == "bot"
    assert "Acceptance Criteria" in agent.prompts[0]
    assert "CI passes" in agent.prompts[0]
    assert tracker.comments == [("issue-id", "Symphony completed `ENG-1` on branch `bot/eng-1`.")]


def test_poll_updates_linear_state_when_configured(tmp_path: Path) -> None:
    config = make_config(tmp_path)
    config.completed_state_id = "done-state"
    issue = LinearIssue("issue-id", "ENG-1", "Update state", "", "Ready")
    tracker = FakeTracker([issue])

    SymphonyOrchestrator(
        config,
        tracker=tracker,
        workflow=WorkflowDefinition(repoUrl="git@example.com:org/repo.git"),
        workspace_manager=FakeWorkspaceManager(tmp_path / "workspace"),
        agent_runner=FakeAgentRunner(),
        state_store=StateStore(tmp_path / "state.json"),
    ).poll_once()

    assert tracker.state_updates == [("issue-id", "done-state")]


def test_poll_skips_open_blocker(tmp_path: Path) -> None:
    expected_skips = 2
    blocked = LinearIssue("blocked-id", "ENG-1", "Blocked", "", "Ready")
    blocker = LinearIssue("blocker-id", "ENG-2", "Blocker", "", "In Progress")
    workflow = WorkflowDefinition(
        repoUrl="git@example.com:org/repo.git",
        blocked_by=["ENG-2"],
        runnableStateNames=["Ready"],
        doneStateNames=["Done"],
    )
    agent = FakeAgentRunner()

    summary = SymphonyOrchestrator(
        make_config(tmp_path),
        tracker=FakeTracker([blocked, blocker]),
        workflow=workflow,
        workspace_manager=FakeWorkspaceManager(tmp_path / "workspace"),
        agent_runner=agent,
        state_store=StateStore(tmp_path / "state.json"),
    ).poll_once()

    assert summary.started == 0
    assert summary.skipped == expected_skips
    assert agent.prompts == []


def test_poll_does_not_duplicate_completed_run(tmp_path: Path) -> None:
    issue = LinearIssue("issue-id", "ENG-1", "Done once", "", "Ready")
    tracker = FakeTracker([issue])
    agent = FakeAgentRunner()
    state = StateStore(tmp_path / "state.json")
    state_record = RunRecord(
        issue_id="issue-id",
        issue_identifier="ENG-1",
        branch="bot/eng-1",
        workspace=str(tmp_path / "workspace"),
        attempts=1,
        status="completed",
    )
    state.upsert(state_record)

    assert state_record.status == "completed"
    summary = SymphonyOrchestrator(
        make_config(tmp_path),
        tracker=tracker,
        workflow=WorkflowDefinition(repoUrl="git@example.com:org/repo.git"),
        workspace_manager=FakeWorkspaceManager(tmp_path / "workspace"),
        agent_runner=agent,
        state_store=state,
    ).poll_once()

    assert summary.started == 0
    assert summary.skipped == 1
    assert agent.prompts == []


def test_retry_counter_advances_when_workspace_prepare_fails(tmp_path: Path) -> None:
    expected_attempts = 2

    class FailingWorkspaceManager(FakeWorkspaceManager):
        def prepare(
            self,
            *,
            issue_identifier: str,
            repo_url: str,
            base_branch: str,
            branch_prefix: str,
        ) -> Workspace:
            raise RuntimeError("clone failed")

    issue = LinearIssue("issue-id", "ENG-1", "Clone fails", "", "Ready")
    tracker = FakeTracker([issue])
    state = StateStore(tmp_path / "state.json")
    orchestrator = SymphonyOrchestrator(
        make_config(tmp_path),
        tracker=tracker,
        workflow=WorkflowDefinition(repoUrl="git@example.com:org/repo.git"),
        workspace_manager=FailingWorkspaceManager(tmp_path / "workspace"),
        agent_runner=FakeAgentRunner(),
        state_store=state,
    )

    first = orchestrator.poll_once()
    second = orchestrator.poll_once()

    assert first.failed == 1
    assert second.failed == 1
    assert state.get("issue-id").attempts == expected_attempts
    assert "retry limit reached" in tracker.comments[-1][1]
