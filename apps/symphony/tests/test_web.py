from pathlib import Path

from fastapi.testclient import TestClient

from symphony.config import SymphonyConfig
from symphony.web import create_app

OK = 200


def test_web_api_creates_agent_and_task(tmp_path: Path) -> None:
    config = SymphonyConfig.model_validate(
        {
            "linearViewId": "view",
            "workflowFile": str(tmp_path / "workflow.md"),
            "workspaceBase": str(tmp_path / "workspaces"),
            "stateFile": str(tmp_path / "state.json"),
            "agentCommand": ["agent"],
            "agentStartIntervalSeconds": 0,
        }
    )
    with TestClient(create_app(config)) as client:
        agent = client.post(
            "/api/agents",
            json={"name": "Reviewer", "command": ["codex", "exec", "--stdin"], "role": "review"},
        )
        assert agent.status_code == OK
        task = client.post(
            "/api/tasks",
            json={"title": "Build UI", "description": "Implement the UI"},
        )
        assert task.status_code == OK
        assert task.json()["status"] == "created"
        assert len(client.get("/api/agents").json()) == 1
        assert len(client.get("/api/tasks").json()) == 1
