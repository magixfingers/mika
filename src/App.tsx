import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { Agent, AgentState, SessionStatus, Snapshot } from "./types";
import { Mika, STATE_LABELS } from "./Mika";
import "./App.css";

// 10 minutes of history at the 2-second sample interval.
const HISTORY_LENGTH = 300;

// Most urgent last, matching AgentState's order in status.rs.
const URGENCY: AgentState[] = ["idle", "done", "thinking", "working", "waiting"];

function mostUrgent(statuses: SessionStatus[]): SessionStatus | null {
  return statuses.reduce<SessionStatus | null>(
    (top, s) => (top === null || URGENCY.indexOf(s.state) > URGENCY.indexOf(top.state) ? s : top),
    null,
  );
}

function describe(status: SessionStatus): string {
  return status.state === "working" && status.tool
    ? `${STATE_LABELS.working} · ${status.tool}`
    : STATE_LABELS[status.state];
}

const AGENT_LABELS: Record<Agent["kind"], string> = {
  claude: "Claude Code",
  codex: "Codex",
};

function formatBytes(bytes: number): string {
  if (bytes >= 1024 ** 3) return `${(bytes / 1024 ** 3).toFixed(1)} GB`;
  if (bytes >= 1024 ** 2) return `${(bytes / 1024 ** 2).toFixed(0)} MB`;
  return `${(bytes / 1024).toFixed(0)} KB`;
}

function formatUptime(secs: number): string {
  const h = Math.floor(secs / 3600);
  const m = Math.floor((secs % 3600) / 60);
  if (h > 0) return `${h}h ${m}m`;
  if (m > 0) return `${m}m`;
  return `${secs}s`;
}

function shortPath(path: string | null): string {
  if (!path) return "unknown folder";
  return path.replace(/^\/Users\/[^/]+/, "~");
}

function Sparkline({ values }: { values: number[] }) {
  if (values.length < 2) return <svg className="sparkline" />;
  const max = Math.max(100, ...values);
  const step = 120 / (HISTORY_LENGTH - 1);
  const offset = 120 - step * (values.length - 1);
  const points = values
    .map((v, i) => `${(offset + i * step).toFixed(1)},${(28 - (v / max) * 26).toFixed(1)}`)
    .join(" ");
  return (
    <svg className="sparkline" viewBox="0 0 120 30" preserveAspectRatio="none">
      <polyline points={points} />
    </svg>
  );
}

function AgentCard({
  agent,
  history,
  status,
}: {
  agent: Agent;
  history: number[];
  status: SessionStatus | null;
}) {
  const [expanded, setExpanded] = useState(false);
  const childCount = agent.processes.length - 1;

  return (
    <section className={`agent agent-${agent.kind}`}>
      <button className="agent-header" onClick={() => setExpanded(!expanded)}>
        <div className="agent-title">
          {status && <Mika state={status.state} size={28} />}
          <div className="agent-name">
            <span className="agent-kind">{AGENT_LABELS[agent.kind]}</span>
            <span className="agent-cwd">
              {status ? `${describe(status)} · ` : ""}
              {shortPath(agent.cwd)}
            </span>
          </div>
        </div>
        <Sparkline values={history} />
        <dl className="agent-stats">
          <div>
            <dt>CPU</dt>
            <dd>{agent.cpu.toFixed(0)}%</dd>
          </div>
          <div>
            <dt>Memory</dt>
            <dd>{formatBytes(agent.memory)}</dd>
          </div>
          <div>
            <dt>Up</dt>
            <dd>{formatUptime(agent.uptime_secs)}</dd>
          </div>
          <div>
            <dt>Procs</dt>
            <dd>{agent.processes.length}</dd>
          </div>
        </dl>
      </button>

      {expanded && (
        <table className="proc-tree">
          <thead>
            <tr>
              <th>Process</th>
              <th>PID</th>
              <th>CPU</th>
              <th>Memory</th>
            </tr>
          </thead>
          <tbody>
            {agent.processes.map((p) => (
              <tr key={p.pid} title={p.command}>
                <td style={{ paddingLeft: `${8 + p.depth * 16}px` }}>
                  {p.depth > 0 && <span className="tree-branch">└ </span>}
                  {p.name}
                </td>
                <td className="num">{p.pid}</td>
                <td className="num">{p.cpu.toFixed(1)}%</td>
                <td className="num">{formatBytes(p.memory)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      {!expanded && childCount > 0 && (
        <p className="agent-hint">
          {childCount} child process{childCount === 1 ? "" : "es"} — click to show
        </p>
      )}
    </section>
  );
}

function App() {
  const [snapshot, setSnapshot] = useState<Snapshot | null>(null);
  const [history, setHistory] = useState<Record<number, number[]>>({});
  const [statuses, setStatuses] = useState<SessionStatus[]>([]);

  useEffect(() => {
    const apply = (next: Snapshot) => {
      setSnapshot(next);
      setHistory((prev) => {
        const updated: Record<number, number[]> = {};
        for (const agent of next.agents) {
          updated[agent.pid] = [...(prev[agent.pid] ?? []), agent.cpu].slice(-HISTORY_LENGTH);
        }
        return updated;
      });
    };

    invoke<Snapshot | null>("latest_snapshot").then((s) => s && apply(s));
    invoke<SessionStatus[]>("session_statuses").then(setStatuses);
    const unlisteners = [
      listen<Snapshot>("agents", (event) => apply(event.payload)),
      listen<SessionStatus[]>("statuses", (event) => setStatuses(event.payload)),
    ];
    return () => {
      unlisteners.forEach((u) => u.then((fn) => fn()));
    };
  }, []);

  const agents = snapshot?.agents ?? [];
  const totalCpu = agents.reduce((sum, a) => sum + a.cpu, 0);
  const totalMemory = agents.reduce((sum, a) => sum + a.memory, 0);
  // Hooks report a session's folder, so a card shows the most urgent session in its folder.
  const statusFor = (agent: Agent) =>
    agent.kind === "claude" ? mostUrgent(statuses.filter((s) => s.cwd === agent.cwd)) : null;
  const overall = mostUrgent(agents.flatMap((a) => statusFor(a) ?? []));

  return (
    <main className="app">
      <header className="app-header">
        <div className="app-title">
          <Mika state={overall?.state ?? "idle"} />
          <div>
            <h1>Mika</h1>
            <p className="summary">{overall ? describe(overall) : "No status from hooks yet"}</p>
          </div>
        </div>
        <p className="summary">
          {snapshot === null
            ? "Sampling…"
            : `${agents.length} agent${agents.length === 1 ? "" : "s"} · ${totalCpu.toFixed(0)}% CPU · ${formatBytes(totalMemory)}`}
        </p>
      </header>

      {snapshot !== null && agents.length === 0 && (
        <p className="empty">No Claude Code or Codex sessions running.</p>
      )}

      {agents.map((agent) => (
        <AgentCard
          key={agent.pid}
          agent={agent}
          history={history[agent.pid] ?? []}
          status={statusFor(agent)}
        />
      ))}
    </main>
  );
}

export default App;
