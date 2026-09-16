import { FormEvent, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

type Overview = {
  product: string;
  autonomy: string;
  deployment: string;
  runtime: string;
  policy: string;
  version: string;
};

type AgentRunResponse = {
  backend: string;
  termination: string;
  stdout: string;
  stderr: string;
  duration_ms: number;
};

type AgentForm = {
  backend: string;
  prompt: string;
  workdir: string;
  model: string;
  allowEnv: string;
  timeoutMs: string;
};

const NAV = [
  "Command Center",
  "Goals",
  "Agent Fleet",
  "Workspace",
  "Terminal",
  "Verification",
  "Models",
  "MCP & Skills",
  "Memory",
  "Policy",
  "Audit",
];

const ROLES = [
  ["DIR", "Director", "Orchestrates the goal graph", "online"],
  ["ARC", "Architect", "Interfaces and decomposition", "ready"],
  ["COD", "Coder", "Implementation worker", "ready"],
  ["DBG", "Debugger", "Root-cause and repair", "ready"],
  ["TST", "Tester", "Evidence and regression", "ready"],
  ["REV", "Reviewer", "Independent cross-review", "ready"],
  ["SEC", "Security", "Policy and risk review", "ready"],
  ["REL", "Release", "Delivery and release gate", "guarded"],
];

const defaultOverview: Overview = {
  product: "OPEN AUTOPILOT",
  autonomy: "Full Autonomy",
  deployment: "Hybrid local-first",
  runtime: "Native Windows",
  policy: "Secure defaults",
  version: "0.1.0",
};

export default function App() {
  const [overview, setOverview] = useState<Overview>(defaultOverview);
  const [backends, setBackends] = useState<string[]>(["OpenCode", "Codex", "Claude", "Gemini"]);
  const [activeNav, setActiveNav] = useState("Command Center");
  const [running, setRunning] = useState(false);
  const [status, setStatus] = useState("Ready for autonomous goal");
  const [result, setResult] = useState<AgentRunResponse | null>(null);
  const [error, setError] = useState("");
  const [form, setForm] = useState<AgentForm>({
    backend: "opencode",
    prompt: "Inspect this repository, identify the highest-impact engineering problem, implement a verified fix, and report the evidence.",
    workdir: "",
    model: "",
    allowEnv: "",
    timeoutMs: "1800000",
  });

  useEffect(() => {
    void invoke<Overview>("system_overview").then(setOverview).catch(() => undefined);
    void invoke<string[]>("list_backends").then(setBackends).catch(() => undefined);
  }, []);

  const output = useMemo(() => {
    if (error) return error;
    if (!result) return "No run output yet. OPEN AUTOPILOT will stream richer events in the next desktop milestone.";
    return [
      `backend=${result.backend}`,
      `termination=${result.termination}`,
      `duration_ms=${result.duration_ms}`,
      "",
      result.stdout,
      result.stderr ? `\n[stderr]\n${result.stderr}` : "",
    ].join("\n");
  }, [error, result]);

  function patch<K extends keyof AgentForm>(key: K, value: AgentForm[K]) {
    setForm((current) => ({ ...current, [key]: value }));
  }

  async function runAgent(event: FormEvent) {
    event.preventDefault();
    setError("");
    setResult(null);
    setRunning(true);
    setStatus("Agent running under local policy controls");
    try {
      const allowEnv = form.allowEnv.split(",").map((item) => item.trim()).filter(Boolean);
      const timeoutMs = form.timeoutMs.trim() ? Number(form.timeoutMs) : null;
      const response = await invoke<AgentRunResponse>("run_agent", {
        request: {
          backend: form.backend,
          prompt: form.prompt,
          workdir: form.workdir,
          model: form.model.trim() || null,
          allow_env: allowEnv,
          timeout_ms: timeoutMs,
        },
      });
      setResult(response);
      setStatus(response.termination === "Exited" ? "Run completed" : `Run ended: ${response.termination}`);
    } catch (reason) {
      setError(String(reason));
      setStatus("Run failed or was interrupted");
    } finally {
      setRunning(false);
    }
  }

  async function stopAll() {
    setStatus("Emergency stop requested");
    try {
      const message = await invoke<string>("stop_all");
      setStatus(message);
    } catch (reason) {
      setError(String(reason));
    }
  }

  return (
    <div className="app-shell">
      <aside className="sidebar">
        <div className="brand">
          <div className="brand-mark">OA</div>
          <div>
            <strong>OPEN AUTOPILOT</strong>
            <span>Autonomous Engineering</span>
          </div>
        </div>
        <nav>
          {NAV.map((item) => (
            <button key={item} className={activeNav === item ? "nav-item active" : "nav-item"} onClick={() => setActiveNav(item)}>
              <span className="nav-dot" />
              {item}
            </button>
          ))}
        </nav>
        <div className="sidebar-foot">
          <span className="pulse" /> Local control plane
          <small>v{overview.version}</small>
        </div>
      </aside>

      <main className="main-panel">
        <header className="topbar">
          <div>
            <div className="eyebrow">{activeNav}</div>
            <h1>Autonomous Command Center</h1>
          </div>
          <div className="top-actions">
            <div className="mode-pill"><span className="pulse" /> {overview.autonomy}</div>
            <button className="stop-button" onClick={() => void stopAll()}>STOP</button>
          </div>
        </header>

        <section className="status-grid">
          <StatusCard label="DIRECTOR" value="ONLINE" detail="Deterministic goal state machine" accent="green" />
          <StatusCard label="DEPLOYMENT" value={overview.deployment} detail="Secrets and execution stay local" />
          <StatusCard label="POLICY GUARD" value="ENFORCED" detail={overview.policy} accent="green" />
          <StatusCard label="RUNTIME" value={overview.runtime} detail="WSL2 / Docker adapters: M3" />
        </section>

        <section className="workspace-grid">
          <div className="card launch-card">
            <div className="card-heading">
              <div>
                <div className="eyebrow">GOAL LAUNCHER</div>
                <h2>Give the agent an objective</h2>
              </div>
              <div className={running ? "run-indicator running" : "run-indicator"}>{running ? "RUNNING" : "READY"}</div>
            </div>

            <form onSubmit={(event) => void runAgent(event)}>
              <div className="field-row">
                <label>
                  Backend
                  <select value={form.backend} onChange={(e) => patch("backend", e.target.value)}>
                    {backends.map((backend) => <option value={backend.toLowerCase().replace(" ", "-")} key={backend}>{backend}</option>)}
                  </select>
                </label>
                <label>
                  Model override
                  <input value={form.model} onChange={(e) => patch("model", e.target.value)} placeholder="Auto / provider default" />
                </label>
              </div>
              <label>
                Repository / work directory
                <input value={form.workdir} onChange={(e) => patch("workdir", e.target.value)} placeholder="C:\\dev\\my-project" required />
              </label>
              <label>
                Objective
                <textarea value={form.prompt} onChange={(e) => patch("prompt", e.target.value)} rows={6} required />
              </label>
              <div className="field-row">
                <label>
                  Explicit environment allowlist
                  <input value={form.allowEnv} onChange={(e) => patch("allowEnv", e.target.value)} placeholder="OPENAI_API_KEY, GH_TOKEN" />
                </label>
                <label>
                  Timeout (ms)
                  <input value={form.timeoutMs} onChange={(e) => patch("timeoutMs", e.target.value)} inputMode="numeric" />
                </label>
              </div>
              <div className="form-footer">
                <div className="status-copy"><span className={running ? "pulse hot" : "pulse"} /> {status}</div>
                <button className="primary-button" type="submit" disabled={running}>{running ? "Agent is working…" : "RUN AUTOPILOT"}</button>
              </div>
            </form>
          </div>

          <div className="card terminal-card">
            <div className="card-heading compact">
              <div>
                <div className="eyebrow">EXECUTION OUTPUT</div>
                <h2>Agent console</h2>
              </div>
              <span className="terminal-badge">LOCAL</span>
            </div>
            <pre>{output}</pre>
          </div>
        </section>

        <section className="card fleet-card">
          <div className="card-heading">
            <div>
              <div className="eyebrow">AGENT FLEET</div>
              <h2>Director + specialist workers</h2>
            </div>
            <span className="subtle">Parallel worktrees · Cross-review · Evidence gates</span>
          </div>
          <div className="fleet-grid">
            {ROLES.map(([code, role, detail, roleStatus]) => (
              <article className="agent-card" key={role}>
                <div className="agent-avatar">{code}</div>
                <div className="agent-copy">
                  <strong>{role}</strong>
                  <span>{detail}</span>
                </div>
                <span className={`agent-status ${roleStatus}`}>{roleStatus}</span>
              </article>
            ))}
          </div>
        </section>
      </main>
    </div>
  );
}

function StatusCard({ label, value, detail, accent }: { label: string; value: string; detail: string; accent?: "green" }) {
  return (
    <article className="status-card">
      <span className="eyebrow">{label}</span>
      <strong className={accent === "green" ? "green" : ""}>{value}</strong>
      <small>{detail}</small>
    </article>
  );
}
