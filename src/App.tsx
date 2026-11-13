import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { Agent, AgentState, ClaudeLimits, SessionStatus, Snapshot, TokenStats } from "./types";
import { Mika, STATE_LABELS } from "./Mika";
import { Activity, Limits, Models, ProviderMark, providerOf, RangePicker, Sessions, SystemPanel, Totals, usageIn, type Range } from "./Usage";
import { compact, formatBytes, formatUptime, shortPath } from "./format";
import { startLiquidGlass, type LiquidGlass } from "./liquid";
import { useEntrance, usePop, useShake, useSlidingPill } from "./motion";
import { useSettings } from "./settings";
import { SettingsPage } from "./SettingsPage";
import "./App.css";

// 5 minutes of history at the 1-second sample interval.
const HISTORY_LENGTH = 300;

// Most urgent last, matching AgentState's order in status.rs.
const URGENCY: AgentState[] = ["idle", "done", "thinking", "working", "turn", "waiting"];

function mostUrgent(statuses: SessionStatus[]): SessionStatus | null {
  return statuses.reduce<SessionStatus | null>(
    (top, s) => (top === null || URGENCY.indexOf(s.state) > URGENCY.indexOf(top.state) ? s : top),
    null,
  );
}

// An agent averaging this much CPU over its last few samples counts as working,
// for agents without hooks (Codex, or Claude sessions started before hooks).
const BUSY_CPU = 5;
const BUSY_SAMPLES = 3;

function isBusy(history: number[] | undefined): boolean {
  const recent = (history ?? []).slice(-BUSY_SAMPLES);
  return recent.length > 0 && recent.reduce((a, b) => a + b, 0) / recent.length >= BUSY_CPU;
}

/** A working status inferred from activity; `tool` carries what was busy. */
function inferredWorking(id: string, agent: Agent["kind"], cwd: string | null, what: string): SessionStatus {
  return { session_id: id, agent, cwd, state: "working", tool: what, since_ms: 0 };
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

/** Gear icon, adapted from Lucide's "settings" (ISC licence). */
function GearIcon() {
  return (
    <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
      <path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z" />
      <circle cx="12" cy="12" r="3" />
    </svg>
  );
}

const appName = (path: string) => path.split("/").pop()?.replace(/\.app$/, "") ?? path;

type Page = "overview" | "agents" | "settings";

/**
 * Moves the window when the header or top strip is dragged. data-tauri-drag-region
 * only reacts to the element itself, not its children, so the header's text and
 * character blocked dragging; this handles any press there except on controls.
 */
function dragWindow(e: React.MouseEvent) {
  if (e.button !== 0 || (e.target as HTMLElement).closest("button, a, input, [data-clickable]")) return;
  if (e.detail === 2) {
    getCurrentWindow().toggleMaximize();
    return;
  }
  getCurrentWindow().startDragging();
}

function App() {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const mainRef = useRef<HTMLElement>(null);
  const [page, setPage] = useState<Page>("overview");
  // The page to return to when Settings is closed with the gear.
  const lastPage = useRef<Exclude<Page, "settings">>("overview");
  const showPage = (p: Exclude<Page, "settings">) => {
    lastPage.current = p;
    setPage(p);
  };
  const toggleSettings = () => setPage(page === "settings" ? lastPage.current : "settings");
  const [snapshot, setSnapshot] = useState<Snapshot | null>(null);
  const [history, setHistory] = useState<Record<number, number[]>>({});
  const [statuses, setStatuses] = useState<SessionStatus[]>([]);
  const [tokens, setTokens] = useState<TokenStats | null>(null);
  const [claudeLimits, setClaudeLimits] = useState<ClaudeLimits | null>(null);
  const [range, setRange] = useState<Range>("day");
  const { settings } = useSettings();

  // Open on the user's default range once settings arrive (later changes don't move the view).
  const rangeChosen = useRef(false);
  useEffect(() => {
    if (!settings || rangeChosen.current) return;
    rangeChosen.current = true;
    setRange(settings.default_range);
  }, [settings]);

  // The Liquid Glass skin runs liquid-dom's WebGPU renderer; Classic is CSS only.
  const skin = settings?.skin;
  const glassLook = settings?.glass;
  const lookRef = useRef(glassLook);
  lookRef.current = glassLook;
  const glassRef = useRef<LiquidGlass | null>(null);
  useEffect(() => {
    const canvas = canvasRef.current;
    const main = mainRef.current;
    if (!canvas || !main || skin !== "liquid") return;
    let cancelled = false;
    const report = (message: string) => invoke("log_frontend", { message });
    startLiquidGlass(canvas, main, (m) => report(`WebGPU error: ${m}`), lookRef.current)
      .then((glass) => {
        // Unmounted or switched skin while WebGPU was starting.
        if (cancelled) return glass?.stop();
        glassRef.current = glass;
        document.documentElement.classList.toggle("liquid", glass !== null);
        report(glass ? "liquid glass running on WebGPU" : "WebGPU unavailable, using the Classic skin");
      })
      .catch((err) => report(`liquid glass failed: ${err}`));
    return () => {
      cancelled = true;
      glassRef.current?.stop();
      glassRef.current = null;
      document.documentElement.classList.remove("liquid");
    };
  }, [skin]);
  // Slider changes restyle the running renderer; no restart needed.
  useEffect(() => {
    if (glassLook) glassRef.current?.setLook(glassLook);
  }, [glassLook]);

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

  // Agents the user turned off in settings are left out everywhere.
  const tracked = (provider: "claude" | "codex") =>
    provider === "claude" ? settings?.track_claude !== false : settings?.track_codex !== false;
  const agents = (snapshot?.agents ?? []).filter((a) => tracked(a.kind));
  // Hooks report a session's folder, so an agent shows the most urgent session in its folder.
  // Hook states win when they report activity; otherwise CPU use marks the agent as working.
  const statusFor = (agent: Agent): SessionStatus | null => {
    // Claude runs one process per session, matched by folder; the ChatGPT app's single
    // Codex agent hosts every Codex thread, so it shows the most urgent of them.
    const hook = mostUrgent(
      statuses.filter((s) => s.agent === agent.kind && (agent.kind === "codex" || s.cwd === agent.cwd)),
    );
    if (hook && hook.state !== "idle" && hook.state !== "done") return hook;
    if (isBusy(history[agent.pid])) return inferredWorking(`pid:${agent.pid}`, agent.kind, agent.cwd, AGENT_LABELS[agent.kind]);
    return hook;
  };
  // The app hosting a session: Codex threads all live in the ChatGPT app's one agent;
  // Claude sessions are matched to their agent by folder.
  const agentFor = (s: SessionStatus) =>
    agents.find((a) => a.kind === s.agent && (s.agent === "codex" || a.cwd === s.cwd)) ?? null;
  const tokensFlowing = (tokens?.tokens_per_sec ?? 0) > 0;
  const overall = mostUrgent([
    ...statuses,
    ...agents.flatMap((a) => statusFor(a) ?? []),
    ...(tokensFlowing ? [inferredWorking("tokens", "claude", null, "tokens flowing")] : []),
  ]);
  const alerting = overall?.state === "waiting";
  // Blocked ("Needs you", red) or just the user's turn (calm): either way the
  // header names the folder and clicking it goes straight to the session.
  const yourMove = alerting || overall?.state === "turn";
  const waitingAgent = yourMove && overall ? agentFor(overall) : null;
  const waitingHost = waitingAgent?.host_app ?? null;
  const openWaitingSession = () => {
    if (!waitingHost) return;
    invoke("focus_app", { path: waitingHost, pid: waitingAgent?.pid ?? null });
    invoke("dismiss_alert");
  };

  // Motion: Mika pops on state changes, the header shakes on alerts, panels rise in.
  const mikaRef = usePop<HTMLSpanElement>(overall?.state ?? "idle");
  const titleRef = useShake<HTMLDivElement>(alerting);
  const tabbarRef = useRef<HTMLElement>(null);
  const tabPill = useSlidingPill(tabbarRef, '[aria-current="page"]', page);
  useEntrance(mainRef, ".totals, .panel", `${page}:${tokens !== null}`);

  // Alerts (the popup, its ringtone and "agent finished") fire from the Rust core
  // when a hook arrives, so they work even while this window is hidden.

  const usage = useMemo(
    () => usageIn(tokens?.days ?? [], range, (model) => tracked(providerOf(model))),
    [tokens, range, settings?.track_claude, settings?.track_codex],
  );

  return (
    <>
      <div className="backdrop" aria-hidden />
      <canvas className="liquid-canvas" ref={canvasRef} aria-hidden />
      {/* The strip beside the traffic lights, where a title bar would be. */}
      <div className="drag-strip" onMouseDown={dragWindow} aria-hidden />
      <main className="app" ref={mainRef}>
        <header className="app-header" onMouseDown={dragWindow}>
          <div
            className="app-title"
            data-alert={alerting || undefined}
            data-clickable={waitingHost ? "" : undefined}
            ref={titleRef}
            role={waitingHost ? "button" : undefined}
            tabIndex={waitingHost ? 0 : undefined}
            title={waitingHost ? `Open in ${appName(waitingHost)}` : undefined}
            onClick={openWaitingSession}
            onKeyDown={(e) => e.key === "Enter" && openWaitingSession()}
          >
            <span className="mika-slot" ref={mikaRef}>
              <Mika state={overall?.state ?? "idle"} size={40} />
            </span>
            <div>
              <h1>Mika</h1>
              <p className="muted small status-line" role={alerting ? "alert" : undefined}>
                {overall
                  ? yourMove
                    ? `${STATE_LABELS[overall.state]} · ${shortPath(overall.cwd)}`
                    : describe(overall)
                  : "Watching for agents"}
              </p>
            </div>
          </div>
          <div className="header-controls">
            {page !== "settings" && <RangePicker value={range} onChange={setRange} />}
            <button
              className="icon-button"
              onClick={toggleSettings}
              aria-label="Settings"
              aria-pressed={page === "settings"}
              title="Settings"
            >
              <GearIcon />
            </button>
          </div>
        </header>

        <div className="pages" data-page={page}>
          <div className="page page-overview">
            {snapshot && <SystemPanel system={snapshot.system} />}
            {tokens === null ? (
              <p className="muted loading">Reading session logs…</p>
            ) : (
              <>
                <Totals usage={usage} range={range} />
                <Limits
                  codex={tracked("codex") ? tokens.codex_limit : null}
                  claude={tracked("claude") && settings?.claude_limits !== false ? claudeLimits : null}
                />
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
            {tokens && (
              <Sessions
                key={range}
                sessions={tokens.sessions[range].filter((s) => tracked(s.provider))}
                reviews={tracked("codex") ? tokens.reviews[range] : null}
                statuses={statuses}
              />
            )}
          </div>

          <div className="page page-settings">
            <SettingsPage />
          </div>
        </div>

        <nav className="tabbar" aria-label="Pages" ref={tabbarRef}>
          <span className="pill" ref={tabPill} aria-hidden />
          {(["overview", "agents"] as const).map((p) => (
            <button key={p} aria-current={page === p ? "page" : undefined} onClick={() => showPage(p)}>
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
