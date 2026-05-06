from __future__ import annotations

import logging
import time
from dataclasses import dataclass

from symphony.agent import AgentResult, AgentRunner
from symphony.config import SymphonyConfig
from symphony.linear import IssueTracker, LinearClient, LinearIssue
from symphony.state import RunRecord, StateStore
from symphony.workflow import WorkflowDefinition, load_workflow
from symphony.workspace import WorkspaceManager

LOGGER = logging.getLogger(__name__)


@dataclass(frozen=True)
class PollSummary:
    started: int = 0
    skipped: int = 0
    failed: int = 0


class SymphonyOrchestrator:
    def __init__(
        self,
        config: SymphonyConfig,
        *,
        tracker: IssueTracker | None = None,
        workflow: WorkflowDefinition | None = None,
        workspace_manager: WorkspaceManager | None = None,
        agent_runner: AgentRunner | None = None,
        state_store: StateStore | None = None,
    ) -> None:
        self.config = config
        self.workflow = workflow or load_workflow(config.workflow_file)
        self.tracker = tracker or LinearClient(config.linear_api_key)
        self.workspace_manager = workspace_manager or WorkspaceManager(config.workspace_base)
        self.agent_runner = agent_runner or AgentRunner(config.agent_command)
        self.state_store = state_store or StateStore(config.resolved_state_file)

    def run_forever(self) -> None:
        while True:
            self.poll_once()
            time.sleep(self.config.poll_interval_seconds)

    def poll_once(self) -> PollSummary:
        summary = PollSummary()
        issues = self.tracker.list_view_issues(self.config.linear_view_id)
        issue_by_id = {issue.id: issue for issue in issues}

        for issue in issues:
            if summary.started >= self.config.max_concurrency:
                break
            if not self._is_candidate(issue, issue_by_id):
                summary = PollSummary(summary.started, summary.skipped + 1, summary.failed)
                continue
            try:
                self._start_issue(issue)
                summary = PollSummary(summary.started + 1, summary.skipped, summary.failed)
            except Exception as exc:
                LOGGER.exception("failed to start issue %s", issue.identifier)
                self._record_failure(issue, str(exc))
                summary = PollSummary(summary.started, summary.skipped, summary.failed + 1)
        return summary

    def _is_candidate(self, issue: LinearIssue, issue_by_id: dict[str, LinearIssue]) -> bool:
        existing = self.state_store.get(issue.id)
        if existing and existing.status in {"running", "completed"}:
            return False
        if existing and existing.attempts > self.config.max_retries:
            return False
        if issue.state_name not in self.workflow.runnable_state_names:
            return False
        return not self._has_open_blocker(issue_by_id)

    def _has_open_blocker(self, issue_by_id: dict[str, LinearIssue]) -> bool:
        done_states = set(self.workflow.done_state_names)
        for blocker in self.workflow.blocked_by:
            blocker_issue = self._find_issue(blocker, issue_by_id)
            if blocker_issue is None or blocker_issue.state_name not in done_states:
                return True
        return False

    def _start_issue(self, issue: LinearIssue) -> None:
        repo_url = self.workflow.repo_url or self.config.default_repo_url
        if not repo_url:
            raise ValueError(
                "repoUrl must be set in workflow front matter or defaultRepoUrl in config"
            )

        branch_prefix = self.workflow.effective_branch_prefix or self.config.branch_prefix
        workspace = self.workspace_manager.prepare(
            issue_identifier=issue.identifier,
            repo_url=repo_url,
            base_branch=self.workflow.base_branch,
            branch_prefix=branch_prefix,
        )
        existing = self.state_store.get(issue.id)
        attempts = existing.attempts + 1 if existing else 1
        self.state_store.upsert(
            RunRecord(
                issue_id=issue.id,
                issue_identifier=issue.identifier,
                branch=workspace.branch,
                workspace=str(workspace.path),
                attempts=attempts,
                status="running",
            )
        )

        prompt = self._build_prompt(issue, workspace.branch)
        if self.config.dry_run:
            result = AgentResult(exit_code=0, stdout="dry run", stderr="")
        else:
            result = self.agent_runner.run(workspace=workspace.path, prompt=prompt)

        if result.ok:
            self.state_store.upsert(
                RunRecord(
                    issue_id=issue.id,
                    issue_identifier=issue.identifier,
                    branch=workspace.branch,
                    workspace=str(workspace.path),
                    attempts=attempts,
                    status="completed",
                )
            )
            self.tracker.comment_issue(
                issue.id,
                f"Symphony completed `{issue.identifier}` on branch `{workspace.branch}`.",
            )
            if self.config.completed_state_id:
                self.tracker.set_issue_state(issue.id, self.config.completed_state_id)
            return

        self._record_failure(issue, result.stderr or result.stdout or "agent failed")

    def _record_failure(self, issue: LinearIssue, error: str) -> None:
        existing = self.state_store.get(issue.id)
        if existing is None:
            attempts = 1
        elif existing.status == "running":
            attempts = existing.attempts
        else:
            attempts = existing.attempts + 1
        record = RunRecord(
            issue_id=issue.id,
            issue_identifier=issue.identifier,
            branch=existing.branch if existing else "",
            workspace=existing.workspace if existing else "",
            attempts=attempts,
            status="failed",
            last_error=error[:2000],
        )
        self.state_store.upsert(record)
        retry_note = (
            "will retry on the next poll"
            if attempts <= self.config.max_retries
            else "retry limit reached"
        )
        self.tracker.comment_issue(
            issue.id,
            f"Symphony failed `{issue.identifier}`: {error[:1000]}\n\n{retry_note}.",
        )
        if attempts > self.config.max_retries and self.config.failure_state_id:
            self.tracker.set_issue_state(issue.id, self.config.failure_state_id)

    def _build_prompt(self, issue: LinearIssue, branch: str) -> str:
        criteria = "\n".join(f"- {item}" for item in self.workflow.acceptance_criteria) or "- None"
        references = "\n".join(f"- {item}" for item in self.workflow.reference_issues) or "- None"
        blockers = "\n".join(f"- {item}" for item in self.workflow.blocked_by) or "- None"
        agent_id = self.workflow.agent_id or self.config.default_agent_id
        return f"""You are Symphony agent `{agent_id}`.

Implement the Linear issue below in the prepared workspace.

Issue: {issue.identifier} - {issue.title}
URL: {issue.url or "n/a"}
Branch: {branch}

## Issue Description
{issue.description or "No Linear description provided."}

## Workflow Issue
{self.workflow.issue or "No workflow issue section provided."}

## Plan
{self.workflow.plan or "No workflow plan section provided."}

## Reference Issues
{references}

## Blocked By
{blockers}

## Acceptance Criteria
{criteria}

When done, leave the workspace on the branch above with changes ready for the caller.
"""

    @staticmethod
    def _find_issue(
        identifier_or_id: str,
        issue_by_id: dict[str, LinearIssue],
    ) -> LinearIssue | None:
        if identifier_or_id in issue_by_id:
            return issue_by_id[identifier_or_id]
        normalized = identifier_or_id.upper()
        for issue in issue_by_id.values():
            if issue.identifier.upper() == normalized:
                return issue
        return None
