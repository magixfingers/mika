import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { LogicalSize } from "@tauri-apps/api/dpi";
import type { Agent, AgentState, ClaudeLimits, SessionStatus, Snapshot, TokenStats } from "./types";
import { Mika, STATE_LABELS } from "./Mika";
import { Activity, Limits, Models, ProviderMark, RangePicker, Sessions, Totals, usageIn, type Range } from "./Usage";
import { compact, formatBytes, formatUptime, shortPath } from "./format";
import { startLiquidGlass } from "./liquid";
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
        <span className="lead">{status ? <Mika state={status.state} size={22} /> : <ProviderMark provider={agent.kind} />}</span>
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

type Page = "overview" | "agents";

function App() {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const mainRef = useRef<HTMLElement>(null);
  const [page, setPage] = useState<Page>("overview");
  const [snapshot, setSnapshot] = useState<Snapshot | null>(null);
  const [history, setHistory] = useState<Record<number, number[]>>({});
  const [statuses, setStatuses] = useState<SessionStatus[]>([]);
  const [tokens, setTokens] = useState<TokenStats | null>(null);
  const [claudeLimits, setClaudeLimits] = useState<ClaudeLimits | null>(null);
  const [range, setRange] = useState<Range>("day");

  useEffect(() => {
    const canvas = canvasRef.current;
    const main = mainRef.current;
    if (!canvas || !main) return;
    let stop: (() => void) | null = null;
    let cancelled = false;
    const report = (message: string) => invoke("log_frontend", { message });
    startLiquidGlass(canvas, main, (m) => report(`WebGPU error: ${m}`))
      .then((s) => {
        // Unmounted while WebGPU was starting.
        if (cancelled) return s?.();
        stop = s;
        document.documentElement.classList.toggle("liquid", s !== null);
        invoke("log_frontend", { message: s ? "liquid glass running on WebGPU" : "WebGPU unavailable, using CSS glass" });
      })
      .catch((err) => invoke("log_frontend", { message: `liquid glass failed: ${err}` }));
    return () => {
      cancelled = true;
      stop?.();
    };
  }, []);

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
    invoke<ClaudeLimits | null>("claude_limits").then(setClaudeLimits);
    const unlisteners = [
      listen<Snapshot>("agents", (event) => apply(event.payload)),
      listen<SessionStatus[]>("statuses", (event) => setStatuses(event.payload)),
      listen<TokenStats>("tokens", (event) => setTokens(event.payload)),
      listen<ClaudeLimits>("claude_limits", (event) => setClaudeLimits(event.payload)),
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

  // On first load, grow the window so the whole overview shows without scrolling.
  const fitted = useRef(false);
  useEffect(() => {
    if (fitted.current || tokens === null) return;
    fitted.current = true;
    requestAnimationFrame(() => {
      const page = mainRef.current?.querySelector<HTMLElement>(".page-overview");
      if (!page) return;
      const missing = page.scrollHeight - page.clientHeight;
      if (missing <= 0) return;
      const height = Math.min(window.innerHeight + missing + 8, window.screen.availHeight - 24);
      getCurrentWindow().setSize(new LogicalSize(window.innerWidth, height));
    });
  }, [tokens]);

  return (
    <>
      <div className="backdrop" aria-hidden />
      <canvas className="liquid-canvas" ref={canvasRef} aria-hidden />
      <main className="app" ref={mainRef}>
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

        <div className="pages" data-page={page}>
          <div className="page page-overview">
            {tokens === null ? (
              <p className="muted loading">Reading session logs…</p>
            ) : (
              <>
                <Totals usage={usage} range={range} />
                <Limits codex={tokens.codex_limit} claude={claudeLimits} />
                <Models usage={usage} />
                <Activity days={tokens.days} />
              </>
            )}
          </div>

          <div className="page page-agents">
            <section className="panel">
              <div className="row">
                <h2>Running</h2>
                <span className="muted small">
                  {agents.length} agent{agents.length === 1 ? "" : "s"}
                </span>
              </div>
              {snapshot !== null && agents.length === 0 && (
                <p className="muted">No Claude Code or Codex sessions running.</p>
              )}
              <ul className="list">
                {agents.map((agent) => (
                  <AgentRow key={agent.pid} agent={agent} history={history[agent.pid] ?? []} status={statusFor(agent)} />
                ))}
              </ul>
            </section>
            {tokens && <Sessions sessions={tokens.sessions} statuses={statuses} />}
          </div>
        </div>

        <nav className="glass tabbar" aria-label="Pages">
          {(["overview", "agents"] as Page[]).map((p) => (
            <button key={p} aria-current={page === p ? "page" : undefined} onClick={() => setPage(p)}>
              {p === "overview" ? "Overview" : `Agents${agents.length ? ` · ${agents.length}` : ""}`}
            </button>
          ))}
          <span className="rate">{compact(tokens?.tokens_per_sec ?? 0)} tok/s</span>
        </nav>
      </main>
    </>
  );
}

export default App;
