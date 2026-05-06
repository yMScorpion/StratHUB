from symphony.workflow import parse_workflow


def test_parse_workflow_front_matter_and_sections() -> None:
    workflow = parse_workflow(
        """---
agentId: codex
repoUrl: git@example.com:org/repo.git
baseBranch: develop
runnableStateNames: [Ready]
doneStateNames: [Done]
---
## Issue
Build the feature.

## Plan
1. Add code.
2. Add tests.

## Reference Issues
- ENG-1

## Blocked By
- ENG-2

## Acceptance Criteria
- Tests pass.
- Docs updated.
"""
    )

    assert workflow.agent_id == "codex"
    assert workflow.repo_url == "git@example.com:org/repo.git"
    assert workflow.base_branch == "develop"
    assert workflow.runnable_state_names == ["Ready"]
    assert workflow.done_state_names == ["Done"]
    assert "Add tests" in workflow.plan
    assert workflow.reference_issues == ["ENG-1"]
    assert workflow.blocked_by == ["ENG-2"]
    assert workflow.acceptance_criteria == ["Tests pass.", "Docs updated."]
