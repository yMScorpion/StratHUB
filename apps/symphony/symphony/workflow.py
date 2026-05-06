from __future__ import annotations

import re
from pathlib import Path
from typing import Any

import yaml
from pydantic import BaseModel, Field

SECTION_RE = re.compile(r"^##\s+(.+?)\s*$", re.MULTILINE)


class WorkflowDefinition(BaseModel):
    agent_id: str | None = Field(default=None, alias="agentId")
    repo_url: str | None = Field(default=None, alias="repoUrl")
    base_branch: str = Field(default="main", alias="baseBranch")
    branch_prefix: str | None = Field(default=None, alias="branchPrefix")
    runnable_state_names: list[str] = Field(
        default_factory=lambda: ["Ready", "Todo", "Backlog"],
        alias="runnableStateNames",
    )
    done_state_names: list[str] = Field(
        default_factory=lambda: ["Done", "Completed", "Merged"],
        alias="doneStateNames",
    )
    failure_state_name: str | None = Field(default=None, alias="failureStateName")
    raw_front_matter: dict[str, Any] = Field(default_factory=dict)
    issue: str = ""
    plan: str = ""
    reference_issues: list[str] = Field(default_factory=list)
    blocked_by: list[str] = Field(default_factory=list)
    acceptance_criteria: list[str] = Field(default_factory=list)
    sections: dict[str, str] = Field(default_factory=dict)

    model_config = {"populate_by_name": True}

    @property
    def effective_branch_prefix(self) -> str | None:
        return self.branch_prefix


def load_workflow(path: str | Path) -> WorkflowDefinition:
    return parse_workflow(Path(path).read_text(encoding="utf-8"))


def parse_workflow(markdown: str) -> WorkflowDefinition:
    front_matter, body = _split_front_matter(markdown)
    sections = _parse_sections(body)
    workflow = WorkflowDefinition.model_validate(
        {
            **front_matter,
            "raw_front_matter": front_matter,
            "issue": sections.get("Issue", ""),
            "plan": sections.get("Plan", ""),
            "reference_issues": _parse_list(sections.get("Reference Issues", "")),
            "blocked_by": _parse_list(sections.get("Blocked By", "")),
            "acceptance_criteria": _parse_list(sections.get("Acceptance Criteria", "")),
            "sections": sections,
        }
    )
    return workflow


def _split_front_matter(markdown: str) -> tuple[dict[str, Any], str]:
    if not markdown.startswith("---\n"):
        return {}, markdown
    _, rest = markdown.split("---\n", 1)
    front_text, body = rest.split("---\n", 1)
    front_matter = yaml.safe_load(front_text) or {}
    if not isinstance(front_matter, dict):
        raise ValueError("workflow front matter must be a YAML mapping")
    return front_matter, body


def _parse_sections(body: str) -> dict[str, str]:
    matches = list(SECTION_RE.finditer(body))
    sections: dict[str, str] = {}
    for index, match in enumerate(matches):
        heading = match.group(1).strip()
        start = match.end()
        end = matches[index + 1].start() if index + 1 < len(matches) else len(body)
        sections[heading] = body[start:end].strip()
    return sections


def _parse_list(text: str) -> list[str]:
    values: list[str] = []
    for raw_line in text.splitlines():
        line = raw_line.strip()
        if not line:
            continue
        if line.startswith(("- ", "* ")):
            line = line[2:].strip()
        values.append(line)
    return values
