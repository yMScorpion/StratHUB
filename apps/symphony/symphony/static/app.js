let currentFilter = "all";
let lastAgents = [];
let lastTasks = [];

async function json(path, options) {
  const response = await fetch(path, {
    headers: { "Content-Type": "application/json" },
    ...options,
  });
  if (!response.ok) throw new Error(await response.text());
  return response.json();
}

function commandTokens(value) {
  return value.split(" ").map((token) => token.trim()).filter(Boolean);
}

function escapeHtml(value) {
  return String(value || "")
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#039;");
}

function initials(name) {
  return String(name || "A")
    .split(/\s+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((part) => part[0].toUpperCase())
    .join("");
}

function statusLabel(status) {
  return String(status || "created").replaceAll("_", " ");
}

function isActive(task) {
  return ["delegated", "running", "reviewing"].includes(task.status);
}

function visibleTasks(tasks) {
  if (currentFilter === "active") return tasks.filter(isActive);
  if (currentFilter === "review") {
    return tasks.filter((task) =>
      ["review", "waiting_human_review", "reviewing", "pr_created"].includes(task.kind) ||
      ["waiting_human_review", "reviewing", "pr_created"].includes(task.status),
    );
  }
  if (currentFilter === "blocked") {
    return tasks.filter((task) => ["failed", "ci_failed"].includes(task.status));
  }
  return tasks;
}

function setMetrics(agents, tasks) {
  document.querySelector("#metric-active").textContent = tasks.filter(isActive).length;
  document.querySelector("#metric-review").textContent = tasks.filter(
    (task) => task.status === "waiting_human_review" || task.status === "reviewing",
  ).length;
  document.querySelector("#metric-prs").textContent = tasks.filter((task) => task.pr_url).length;
  document.querySelector("#metric-agents").textContent = agents.length;
}

function renderAgentSelect(agents) {
  const agentSelect = document.querySelector("#agent-select");
  agentSelect.innerHTML = '<option value="">Auto assign</option>';
  for (const agent of agents) {
    const option = document.createElement("option");
    option.value = agent.id;
    option.textContent = `${agent.name} (${agent.role})`;
    agentSelect.appendChild(option);
  }
}

function renderAgents(agents) {
  const target = document.querySelector("#agents-list");
  if (!agents.length) {
    target.innerHTML = '<div class="empty-state">No agents registered.</div>';
    return;
  }
  target.innerHTML = agents.map((agent) => `
    <div class="agent-card">
      <div class="agent-top">
        <div class="agent-identity">
          <span class="agent-avatar">${escapeHtml(initials(agent.name))}</span>
          <div>
            <strong>${escapeHtml(agent.name)}</strong>
            <div class="meta">
              <span class="pill">${escapeHtml(agent.role)}</span>
              <span class="pill">${agent.max_concurrent_tasks} slot${agent.max_concurrent_tasks === 1 ? "" : "s"}</span>
              <span class="pill">${agent.enabled ? "enabled" : "disabled"}</span>
            </div>
          </div>
        </div>
      </div>
      <div class="code-line">${escapeHtml(agent.command.join(" "))}</div>
    </div>
  `).join("");
}

function renderTasks(tasks) {
  const target = document.querySelector("#tasks-list");
  const filtered = visibleTasks(tasks);
  if (!filtered.length) {
    target.innerHTML = '<div class="empty-state">No tasks in this view.</div>';
    return;
  }
  target.innerHTML = filtered.map((task) => `
    <article class="task-card">
      <div class="task-top">
        <div>
          <strong>${escapeHtml(task.title)}</strong>
          <div class="meta">
            <span class="pill status-${escapeHtml(task.status)}">${escapeHtml(statusLabel(task.status))}</span>
            <span class="pill">${escapeHtml(task.kind)}</span>
            <span class="pill">${escapeHtml(task.agent_id || "auto assign")}</span>
            ${task.branch ? `<span class="pill">${escapeHtml(task.branch)}</span>` : ""}
            ${task.pr_url ? `<a class="pill" href="${escapeHtml(task.pr_url)}" target="_blank" rel="noreferrer">Open PR</a>` : ""}
            ${task.status === "waiting_human_review" ? `<button type="button" data-merge="${escapeHtml(task.id)}">Mark merged</button>` : ""}
          </div>
        </div>
      </div>
      <p>${escapeHtml(task.description)}</p>
      ${task.review_notes ? `<pre>${escapeHtml(task.review_notes)}</pre>` : ""}
      ${task.error ? `<pre>${escapeHtml(task.error)}</pre>` : ""}
    </article>
  `).join("");
}

async function refresh() {
  const [agents, tasks] = await Promise.all([json("/api/agents"), json("/api/tasks")]);
  lastAgents = agents;
  lastTasks = tasks;
  setMetrics(agents, tasks);
  renderAgentSelect(agents);
  renderAgents(agents);
  renderTasks(tasks);
  document.querySelector("#scheduler-status").textContent = "Running";
}

document.querySelector("#refresh").addEventListener("click", refresh);

document.querySelector("#agent-form").addEventListener("submit", async (event) => {
  event.preventDefault();
  const form = new FormData(event.currentTarget);
  await json("/api/agents", {
    method: "POST",
    body: JSON.stringify({
      name: form.get("name"),
      command: commandTokens(form.get("command")),
      role: form.get("role"),
      max_concurrent_tasks: Number(form.get("max_concurrent_tasks") || 1),
    }),
  });
  event.currentTarget.reset();
  event.currentTarget.querySelector('[name="command"]').value = "codex exec --stdin";
  event.currentTarget.querySelector('[name="max_concurrent_tasks"]').value = "1";
  await refresh();
});

document.querySelector("#task-form").addEventListener("submit", async (event) => {
  event.preventDefault();
  const form = new FormData(event.currentTarget);
  await json("/api/tasks", {
    method: "POST",
    body: JSON.stringify({
      title: form.get("title"),
      description: form.get("description"),
      repo_url: form.get("repo_url") || null,
      base_branch: form.get("base_branch") || "main",
      agent_id: form.get("agent_id") || null,
    }),
  });
  event.currentTarget.reset();
  event.currentTarget.querySelector('[name="base_branch"]').value = "main";
  await refresh();
});

document.querySelectorAll(".segmented button").forEach((button) => {
  button.addEventListener("click", () => {
    currentFilter = button.dataset.filter;
    document.querySelectorAll(".segmented button").forEach((item) => {
      item.classList.toggle("active", item === button);
    });
    renderTasks(lastTasks);
  });
});

document.addEventListener("click", async (event) => {
  const target = event.target;
  const taskId = target.dataset && target.dataset.merge;
  if (!taskId) return;
  await json(`/api/tasks/${taskId}/merged`, { method: "POST" });
  await refresh();
});

refresh().catch((error) => {
  document.querySelector("#scheduler-status").textContent = "Offline";
  document.querySelector("#sync-copy").textContent = error.message;
});

setInterval(() => {
  refresh().catch(() => {
    document.querySelector("#scheduler-status").textContent = "Offline";
  });
}, 5000);
