from __future__ import annotations

from dataclasses import dataclass
from typing import Any, Protocol

import httpx


class IssueTracker(Protocol):
    def list_view_issues(self, view_id: str) -> list[LinearIssue]: ...

    def comment_issue(self, issue_id: str, body: str) -> None: ...

    def set_issue_state(self, issue_id: str, state_id: str) -> None: ...


@dataclass(frozen=True)
class LinearIssue:
    id: str
    identifier: str
    title: str
    description: str
    state_name: str
    url: str | None = None
    parent_id: str | None = None

    @classmethod
    def from_node(cls, node: dict[str, Any]) -> LinearIssue:
        state = node.get("state") or {}
        parent = node.get("parent") or {}
        return cls(
            id=node["id"],
            identifier=node.get("identifier", node["id"]),
            title=node.get("title", ""),
            description=node.get("description") or "",
            state_name=state.get("name", ""),
            url=node.get("url"),
            parent_id=parent.get("id"),
        )


class LinearClient:
    def __init__(self, api_key: str, *, base_url: str = "https://api.linear.app/graphql") -> None:
        self._client = httpx.Client(
            base_url=base_url,
            headers={"Authorization": api_key, "Content-Type": "application/json"},
            timeout=30.0,
        )

    def list_view_issues(self, view_id: str) -> list[LinearIssue]:
        query = """
        query SymphonyViewIssues($id: String!) {
          customView(id: $id) {
            issues(first: 100) {
              nodes {
                id
                identifier
                title
                description
                url
                state { name }
                parent { id }
              }
            }
          }
        }
        """
        data = self._graphql(query, {"id": view_id})
        nodes = data["customView"]["issues"]["nodes"]
        return [LinearIssue.from_node(node) for node in nodes]

    def comment_issue(self, issue_id: str, body: str) -> None:
        mutation = """
        mutation SymphonyComment($issueId: String!, $body: String!) {
          commentCreate(input: { issueId: $issueId, body: $body }) {
            success
          }
        }
        """
        self._graphql(mutation, {"issueId": issue_id, "body": body})

    def set_issue_state(self, issue_id: str, state_id: str) -> None:
        mutation = """
        mutation SymphonyIssueState($issueId: String!, $stateId: String!) {
          issueUpdate(id: $issueId, input: { stateId: $stateId }) {
            success
          }
        }
        """
        self._graphql(mutation, {"issueId": issue_id, "stateId": state_id})

    def _graphql(self, query: str, variables: dict[str, Any]) -> dict[str, Any]:
        response = self._client.post("", json={"query": query, "variables": variables})
        response.raise_for_status()
        payload = response.json()
        if payload.get("errors"):
            raise RuntimeError(f"Linear GraphQL error: {payload['errors']}")
        return payload["data"]
