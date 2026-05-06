# Symphony

Symphony watches a Linear custom view for runnable issues, prepares a repository workspace, and invokes a configured coding-agent command with the workflow context described by the Symphony spec.

## Configuration

Create `symphony.json`:

```json
{
  "linearApiKey": "env:LINEAR_API_KEY",
  "linearViewId": "view-id",
  "workflowFile": "workflow.md",
  "workspaceBase": ".symphony/workspaces",
  "agentCommand": ["codex", "exec", "--stdin"],
  "defaultRepoUrl": "git@github.com:org/repo.git",
  "defaultAgentId": "codex",
  "pollIntervalSeconds": 60,
  "maxConcurrency": 1,
  "branchPrefix": "symphony",
  "maxRetries": 1,
  "completedStateId": "linear-done-state-id",
  "failureStateId": "linear-failed-state-id"
}
```

The workflow file is Markdown with YAML front matter and `## Issue`, `## Plan`, `## Reference Issues`, `## Blocked By`, and `## Acceptance Criteria` sections. Front matter can set `repoUrl`, `agentId`, `baseBranch`, `branchPrefix`, `runnableStateNames`, and `doneStateNames`.

Run one poll:

```sh
symphony run --config symphony.json --once
```

Run as a long-lived CLI poller:

```sh
symphony run --config symphony.json
```

## Local Web Console

Symphony also runs as a 24/7 local control plane with a web UI and background scheduler:

```sh
symphony serve --config symphony.json
```

Open `http://127.0.0.1:8765`.

The web console lets you register implementation and review agents, create tasks, assign a task to a specific agent or leave it on auto assignment, and watch each task move through delegation, PR creation, review, fix follow-up, and human-review states.

The scheduler respects these rate-limit controls from `symphony.json`:

```json
{
  "maxAgents": 2,
  "maxAgentStartsPerHour": 12,
  "agentStartIntervalSeconds": 300,
  "autoPublishPrs": true
}
```

Use `docker compose -f infra/compose/docker-compose.yml up --build symphony` to deploy the local web service on port `8765`. The compose service expects `/data/symphony.json`, backed by the repo-local `.symphony/symphony.json` file.
