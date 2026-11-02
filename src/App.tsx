import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { Agent, AgentState, SessionStatus, Snapshot, TokenStats } from "./types";
import { Mika, STATE_LABELS } from "./Mika";
import { Activity, Limits, Models, ProviderMark, RangePicker, Sessions, Totals, usageIn, type Range } from "./Usage";
import { compact, formatBytes, formatUptime, shortPath } from "./format";
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

function AgentRow({ agent, history, status }: { agent: Agent; history: number[]; status: SessionStatus | null }) {
  const [expanded, setExpanded] = useState(false);
  return (
    <li className={`agent agent-${agent.kind}`}>
      <button className="agent-header" onClick={() => setExpanded(!expanded)} aria-expanded={expanded}>
        {status ? <Mika state={status.state} size={22} /> : <ProviderMark provider={agent.kind} />}
        <div className="session-text">
          <span className="name">{AGENT_LABELS[agent.kind]}</span>
          <span className="muted small">
            {status ? `${describe(status)} · ` : ""}
            {shortPath(agent.cwd)}
          </span>
        </div>
        <Sparkline values={history} />
        <div className="agent-stats">
          <span>{agent.cpu.toFixed(0)}%</span>
          <span className="muted small">{formatBytes(agent.memory)}</span>
        </div>
      </button>
      {expanded && (
        <table className="proc-tree">
          <thead>
            <tr>
              <th>Process</th>
              <th>CPU</th>
              <th>Memory</th>
            </tr>
          </thead>
          <tbody>
            {agent.processes.map((p) => (
              <tr key={p.pid} title={`${p.pid} · ${p.command}`}>
                <td style={{ paddingLeft: `${4 + p.depth * 12}px` }}>
                  {p.depth > 0 && <span className="muted">└ </span>}
                  {p.name}
                </td>
                <td className="num">{p.cpu.toFixed(1)}%</td>
                <td className="num">{formatBytes(p.memory)}</td>
              </tr>
            ))}
          </tbody>
          <tfoot>
            <tr>
              <td colSpan={3} className="muted">
                Up {formatUptime(agent.uptime_secs)} · pid {agent.pid}
              </td>
            </tr>
          </tfoot>
        </table>
      )}
    </li>
  );
}

/**
 * Scales `ref`'s content down so all of it fits the window without scrolling.
 * The content is widened by the same factor, so it scales as a whole.
 */
function useFitToWindow() {
  const ref = useRef<HTMLElement>(null);
  const [scale, setScale] = useState(1);
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const fit = () => {
      // offsetHeight is the unscaled layout height; transforms don't change it.
      const next = Math.min(1, window.innerHeight / el.offsetHeight);
      setScale((prev) => (Math.abs(prev - next) > 0.005 ? next : prev));
    };
    const observer = new ResizeObserver(fit);
    observer.observe(el);
    window.addEventListener("resize", fit);
    return () => {
      observer.disconnect();
      window.removeEventListener("resize", fit);
    };
  }, []);
  return { ref, scale };
}

function App() {
  const { ref: fitRef, scale } = useFitToWindow();
  const [snapshot, setSnapshot] = useState<Snapshot | null>(null);
  const [history, setHistory] = useState<Record<number, number[]>>({});
  const [statuses, setStatuses] = useState<SessionStatus[]>([]);
  const [tokens, setTokens] = useState<TokenStats | null>(null);
  const [range, setRange] = useState<Range>("day");

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
    invoke<TokenStats | null>("latest_tokens").then((t) => t && setTokens(t));
    const unlisteners = [
      listen<Snapshot>("agents", (event) => apply(event.payload)),
      listen<SessionStatus[]>("statuses", (event) => setStatuses(event.payload)),
      listen<TokenStats>("tokens", (event) => setTokens(event.payload)),
    ];
    return () => {
      unlisteners.forEach((u) => u.then((fn) => fn()));
    };
  }, []);

  const agents = snapshot?.agents ?? [];
  // Hooks report a session's folder, so an agent shows the most urgent session in its folder.
  const statusFor = (agent: Agent) =>
    agent.kind === "claude" ? mostUrgent(statuses.filter((s) => s.cwd === agent.cwd)) : null;
  const overall = mostUrgent(statuses);
  const usage = useMemo(() => usageIn(tokens?.days ?? [], range), [tokens, range]);

  return (
    <>
      <div className="backdrop" aria-hidden />
      <main
        className="app"
        ref={fitRef}
        style={{ transform: `scale(${scale})`, width: `${100 / scale}vw` }}
      >
        <header className="app-header" data-tauri-drag-region>
          <div className="app-title" data-tauri-drag-region>
            <Mika state={overall?.state ?? "idle"} size={40} />
            <div>
              <h1>Mika</h1>
              <p className="muted small">{overall ? describe(overall) : "Watching for agents"}</p>
            </div>
          </div>
          <RangePicker value={range} onChange={setRange} />
        </header>

        {tokens === null ? (
          <p className="muted loading">Reading session logs…</p>
        ) : (
          <>
            <Totals usage={usage} range={range} />
            <Limits codex={tokens.codex_limit} />
            <Models usage={usage} />
          </>
        )}

        <section className="glass panel">
          <div className="row">
            <h2>Running</h2>
            <span className="muted small">
              {agents.length} agent{agents.length === 1 ? "" : "s"}
            </span>
          </div>
          {snapshot !== null && agents.length === 0 && <p className="muted">No Claude Code or Codex sessions running.</p>}
          <ul className="list">
            {agents.map((agent) => (
              <AgentRow key={agent.pid} agent={agent} history={history[agent.pid] ?? []} status={statusFor(agent)} />
            ))}
          </ul>
        </section>

        {tokens && (
          <>
            <Sessions sessions={tokens.sessions} statuses={statuses} />
            <Activity days={tokens.days} />
          </>
        )}

        <footer className="glass footer">
          <span className="rate">{compact(tokens?.tokens_per_sec ?? 0)} tok/s</span>
          <span className="muted small">last minute</span>
        </footer>
      </main>
    </>
  );
}

export default App;
