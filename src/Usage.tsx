import { useLayoutEffect, useRef, useState } from "react";
import type { ClaudeLimits, DayUsage, ReviewSummary, SystemLoad, Thermal, ModelUsage, Provider, RateLimit, SessionStatus, SessionUsage } from "./types";
import { Mika, STATE_LABELS } from "./Mika";
import { CLAUDE_LOGO, OPENAI_LOGO } from "./logos";
import { useRollingNumber, useSlidingPill } from "./motion";
import { compact, formatCost, localDate, timeUntil } from "./format";

export type Range = "day" | "month" | "total";

const RANGE_LABELS: Record<Range, string> = { day: "Today", month: "This month", total: "All time" };

export function providerOf(model: string): Provider {
  return model.startsWith("claude") ? "claude" : "codex";
}

/** Per-model usage summed over the days in `range`. */
export function usageIn(days: DayUsage[], range: Range, now = new Date()): Map<string, ModelUsage> {
  const today = localDate(now);
  const inRange = (date: string) =>
    range === "total" || (range === "day" ? date === today : date.slice(0, 7) === today.slice(0, 7));
  const out = new Map<string, ModelUsage>();
  for (const day of days.filter((d) => inRange(d.date))) {
    for (const [model, u] of Object.entries(day.models)) {
      const sum = out.get(model) ?? { tokens: 0, cost: 0 };
      out.set(model, { tokens: sum.tokens + u.tokens, cost: sum.cost + u.cost });
    }
  }
  return out;
}

export function RangePicker({ value, onChange }: { value: Range; onChange: (r: Range) => void }) {
  const box = useRef<HTMLDivElement>(null);
  const pill = useSlidingPill(box, '[aria-selected="true"]', value);
  return (
    <div className="glass segmented" role="tablist" aria-label="Time range" ref={box}>
      <span className="pill" ref={pill} aria-hidden />
      {(["day", "month", "total"] as Range[]).map((r) => (
        <button key={r} role="tab" aria-selected={value === r} onClick={() => onChange(r)}>
          {r === "day" ? "Day" : r === "month" ? "Month" : "Total"}
        </button>
      ))}
    </div>
  );
}

// Thermal pressure as a bar: each step up the OS scale fills another quarter.
const THERMAL_LEVEL: Record<Thermal, number> = { normal: 0.25, fair: 0.5, serious: 0.75, critical: 1, unknown: 0 };

function LoadRow({ label, hint, fill, value }: { label: string; hint: string; fill: number; value: React.ReactNode }) {
  return (
    <div className="load-row" title={hint}>
      <span className="load-label">{label}</span>
      <div className="meter" style={{ "--fill": Math.min(1, Math.max(0, fill)) } as React.CSSProperties}>
        <span />
      </div>
      <span className="load-value num">{value}</span>
    </div>
  );
}

export function SystemPanel({ system }: { system: SystemLoad }) {
  const gb = (bytes: number) => `${(bytes / 1024 ** 3).toFixed(1)}G`;
  const ram = system.memory_total ? system.memory_used / system.memory_total : 0;
  const swap = system.swap_total ? system.swap_used / system.swap_total : 0;
  return (
    <section className="panel load">
      <LoadRow label="CPU" hint="Average use across all cores" fill={system.cpu / 100} value={`${Math.round(system.cpu)}%`} />
      <LoadRow
        label="RAM"
        hint={`${gb(system.memory_used)} of ${gb(system.memory_total)} in use`}
        fill={ram}
        value={`${Math.round(ram * 100)}%`}
      />
      <LoadRow
        label="Swap"
        hint="Memory paged out to disk; active while it is growing"
        fill={swap}
        value={
          <>
            <span className={system.swapping ? "warn" : undefined}>{system.swapping ? "active" : "idle"}</span>
            <span className="muted"> · {gb(system.swap_used)}</span>
          </>
        }
      />
      <LoadRow
        label="Temp"
        hint="macOS thermal pressure; above normal, the system slows down to cool"
        fill={THERMAL_LEVEL[system.thermal]}
        value={<span className={system.thermal === "serious" || system.thermal === "critical" ? "warn" : "muted"}>{system.thermal}</span>}
      />
    </section>
  );
}

const formatCount = (n: number) => n.toLocaleString();

export function Totals({ usage, range }: { usage: Map<string, ModelUsage>; range: Range }) {
  const tokens = [...usage.values()].reduce((s, u) => s + u.tokens, 0);
  const cost = [...usage.values()].reduce((s, u) => s + u.cost, 0);
  const rolling = useRollingNumber<HTMLSpanElement>(tokens, formatCount);
  return (
    <section className="totals">
      <p className="eyebrow">{RANGE_LABELS[range]}</p>
      <p className="total-tokens">
        {/* Text is written by useRollingNumber, so React never fights the tween. */}
        <span ref={rolling} />
        <span className="total-compact">≈ {compact(tokens)}</span>
      </p>
      <p className="total-cost" title="Estimated from public API list prices">
        {formatCost(cost)} <span className="muted">est.</span>
      </p>
    </section>
  );
}

const WINDOW_LABELS: Record<string, string> = {
  weekly_all: "Weekly",
  session: "5-hour",
  five_hour: "5-hour",
};

/** `weekly_scoped:Sonnet` → "Weekly · Sonnet"; unknown kinds are tidied up. */
function windowLabel(id: string): string {
  const [kind, scope] = id.split(":");
  const base =
    WINDOW_LABELS[kind] ??
    (kind.startsWith("weekly") ? "Weekly" : kind.replace(/_/g, " ").replace(/^./, (c) => c.toUpperCase()));
  return scope ? `${base} · ${scope}` : base;
}

function LimitMeter({ label, used, resetsAt }: { label: string; used: number; resetsAt: number | null }) {
  const left = Math.min(100, Math.max(0, 100 - used));
  return (
    <div className="limit">
      <div className="meter" style={{ "--fill": left / 100 } as React.CSSProperties}>
        <span />
      </div>
      <div className="row small">
        <span className="muted">
          {label}
          {resetsAt ? ` · resets in ${timeUntil(resetsAt)}` : ""}
        </span>
        <span>{left.toFixed(0)}% left</span>
      </div>
    </div>
  );
}

function LimitHeader({ provider, plan }: { provider: Provider; plan: string | null }) {
  return (
    <div className="row">
      <span className="name">
        <ProviderMark provider={provider} /> {provider === "claude" ? "Claude" : "Codex"}
      </span>
      {plan && <span className="muted plan">{plan}</span>}
    </div>
  );
}

export function Limits({ codex, claude }: { codex: RateLimit | null; claude: ClaudeLimits | null }) {
  if (!codex && !claude?.windows.length) return null;
  return (
    <section className="panel">
      <h2>Limits</h2>
      {codex && (
        <div className="limit-group">
          <LimitHeader provider="codex" plan={codex.plan} />
          <LimitMeter
            label={codex.window_minutes >= 10080 ? "Weekly" : `${Math.round(codex.window_minutes / 60)}-hour`}
            used={codex.used_percent}
            resetsAt={codex.resets_at_ms}
          />
        </div>
      )}
      {claude && claude.windows.length > 0 && (
        <div className="limit-group">
          <LimitHeader provider="claude" plan={claude.plan} />
          {claude.windows.map((w) => (
            <LimitMeter key={w.id} label={windowLabel(w.id)} used={w.used_percent} resetsAt={w.resets_at_ms} />
          ))}
        </div>
      )}
    </section>
  );
}

export function Models({ usage }: { usage: Map<string, ModelUsage> }) {
  const rows = [...usage.entries()]
    .filter(([, u]) => u.tokens > 0)
    .sort((a, b) => b[1].tokens - a[1].tokens)
    .slice(0, 6);
  const total = rows.reduce((s, [, u]) => s + u.tokens, 0) || 1;
  return (
    <section className="panel">
      <h2>Models</h2>
      {rows.length === 0 && <p className="muted">No usage in this range.</p>}
      <ul className="list">
        {rows.map(([model, u]) => (
          <li key={model} className="model">
            <div className="row">
              <span className="name">
                <ProviderMark provider={providerOf(model)} /> {model}
              </span>
              <span className="num">
                {compact(u.tokens)} <span className="muted">{Math.round((u.tokens / total) * 100)}%</span>
              </span>
            </div>
            <div
              className={`share share-${providerOf(model)}`}
              style={{ "--fill": u.tokens / total } as React.CSSProperties}
            />
          </li>
        ))}
      </ul>
    </section>
  );
}

const PROVIDER_LABELS: Record<Provider, string> = { claude: "Claude Code", codex: "Codex" };

function clock(ms: number): string {
  return new Date(ms).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", hour12: false });
}

function SessionRow({ s, live, max }: { s: SessionUsage; live?: SessionStatus; max: number }) {
  const model = s.models.length === 1 ? s.models[0].replace(/-\d{8}$/, "") : `${s.models.length} models`;
  const untitled = !s.title;
  const title = s.title ?? `${PROVIDER_LABELS[s.provider]} · ${model}`;
  const kind = [
    PROVIDER_LABELS[s.provider],
    model,
    s.effort,
    s.subagents > 0 && `${s.subagents} subagent${s.subagents === 1 ? "" : "s"}`,
  ];
  const activity = [
    clock(s.last_active_ms),
    `${s.calls} call${s.calls === 1 ? "" : "s"}`,
    `${Math.round(s.cache_hit * 100)}% cached`,
  ];
  const cacheLeft = s.cache_expires_ms ? Math.ceil((s.cache_expires_ms - Date.now()) / 60000) : 0;
  return (
    <li className="session">
      <span className="lead">{live ? <Mika state={live.state} size={22} /> : <ProviderMark provider={s.provider} />}</span>
      <div className="session-body">
        <div className="row">
          <span className="name">{title}</span>
          <span className="num">{s.tokens.toLocaleString()}</span>
        </div>
        <div className="row small">
          <span className="muted session-meta">
            {(untitled ? [live && STATE_LABELS[live.state], ...activity] : [live && STATE_LABELS[live.state], ...kind])
              .filter(Boolean)
              .join(" · ")}
          </span>
          <span className="muted num">{formatCost(s.cost)}</span>
        </div>
        <div className="row small">
          <span className="muted session-meta">{untitled ? s.id : activity.join(" · ")}</span>
          {s.context_percent !== null ? (
            <span className="context" title="Context window used by the last call">
              <span className="meter mini" style={{ "--fill": Math.min(1, s.context_percent / 100) } as React.CSSProperties}>
                <span />
              </span>
              <span className="muted num">{Math.round(s.context_percent)}%</span>
            </span>
          ) : cacheLeft > 0 ? (
            <span className="muted num" title="Time left on Claude's prompt cache">
              Cache {cacheLeft}m
            </span>
          ) : null}
        </div>
        <div className="share" style={{ "--fill": s.tokens / max } as React.CSSProperties} />
      </div>
    </li>
  );
}

function ReviewRow({ r, max }: { r: ReviewSummary; max: number }) {
  return (
    <li className="session">
      <span className="lead">
        <ProviderMark provider="codex" />
      </span>
      <div className="session-body">
        <div className="row">
          <span className="name">Codex Auto Review</span>
          <span className="num">{r.tokens.toLocaleString()}</span>
        </div>
        <div className="row small">
          <span className="muted session-meta">
            Latest {clock(r.latest_ms)} · {compact(r.latest_tokens)}
          </span>
          <span className="muted num">{formatCost(r.cost)}</span>
        </div>
        <div className="row small">
          <span className="muted session-meta">
            {r.runs} background run{r.runs === 1 ? "" : "s"}
          </span>
        </div>
        <div className="share" style={{ "--fill": r.tokens / max } as React.CSSProperties} />
      </div>
    </li>
  );
}

export function Sessions({
  sessions,
  reviews,
  statuses,
}: {
  sessions: SessionUsage[];
  reviews: ReviewSummary | null;
  statuses: SessionStatus[];
}) {
  // Fade the bottom edge only when rows are actually cut off.
  const listRef = useRef<HTMLUListElement>(null);
  const [clipped, setClipped] = useState(false);
  useLayoutEffect(() => {
    const el = listRef.current;
    if (!el) return;
    const check = () => setClipped(el.scrollHeight > el.clientHeight + 1);
    const observer = new ResizeObserver(check);
    observer.observe(el);
    check();
    return () => observer.disconnect();
  }, [sessions, reviews]);

  // Rows sort by recency; the review summary slots in by its latest run.
  const rows: ({ kind: "session"; s: SessionUsage; at: number } | { kind: "review"; r: ReviewSummary; at: number })[] = [
    ...sessions.map((s) => ({ kind: "session" as const, s, at: s.last_active_ms })),
    ...(reviews ? [{ kind: "review" as const, r: reviews, at: reviews.latest_ms }] : []),
  ].sort((a, b) => b.at - a.at);
  const max = Math.max(1, ...sessions.map((s) => s.tokens), reviews?.tokens ?? 0);

  return (
    <section className="panel sessions">
      <h2>Sessions</h2>
      {rows.length === 0 && <p className="muted">No sessions in this range.</p>}
      <ul className="list" ref={listRef} data-clipped={clipped || undefined}>
        {rows.map((row) =>
          row.kind === "review" ? (
            <ReviewRow key="review" r={row.r} max={max} />
          ) : (
            <SessionRow
              key={`${row.s.provider}:${row.s.id}`}
              s={row.s}
              max={max}
              live={row.s.provider === "claude" ? statuses.find((st) => st.session_id === row.s.id) : undefined}
            />
          ),
        )}
      </ul>
    </section>
  );
}

const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
const TREND_DAYS = 88;

function dailyTotals(days: DayUsage[]): Map<string, number> {
  return new Map(days.map((d) => [d.date, Object.values(d.models).reduce((s, m) => s + m.tokens, 0)]));
}

/** Smooth SVG path through points (Catmull-Rom converted to cubic Béziers). */
function smoothPath(points: [number, number][]): string {
  if (points.length < 2) return "";
  let d = `M${points[0][0]},${points[0][1]}`;
  for (let i = 0; i < points.length - 1; i++) {
    const [p0, p1, p2, p3] = [points[i - 1] ?? points[i], points[i], points[i + 1], points[i + 2] ?? points[i + 1]];
    const c1 = [p1[0] + (p2[0] - p0[0]) / 6, p1[1] + (p2[1] - p0[1]) / 6];
    const c2 = [p2[0] - (p3[0] - p1[0]) / 6, p2[1] - (p3[1] - p1[1]) / 6];
    d += ` C${c1[0].toFixed(1)},${c1[1].toFixed(1)} ${c2[0].toFixed(1)},${c2[1].toFixed(1)} ${p2[0].toFixed(1)},${p2[1].toFixed(1)}`;
  }
  return d;
}

export function Activity({ days }: { days: DayUsage[] }) {
  const totals = dailyTotals(days);
  const today = new Date();
  today.setHours(0, 0, 0, 0);

  // Year to date: columns are weeks from the one containing Jan 1, rows run Sunday to Saturday.
  const start = new Date(today.getFullYear(), 0, 1);
  start.setDate(start.getDate() - start.getDay());
  const weeks = Math.floor((today.getTime() - start.getTime()) / (7 * 86400000)) + 1;
  const cells: { date: string; tokens: number; hidden: boolean }[] = [];
  const monthStarts: { month: number; week: number }[] = [];
  for (let i = 0; i < weeks * 7; i++) {
    const d = new Date(start);
    d.setDate(start.getDate() + i);
    if (d.getDate() === 1) monthStarts.push({ month: d.getMonth(), week: Math.floor(i / 7) });
    const date = localDate(d);
    cells.push({ date, tokens: totals.get(date) ?? 0, hidden: d > today || d.getFullYear() !== today.getFullYear() });
  }
  const max = Math.max(1, ...cells.map((c) => c.tokens));
  const active = [...totals.values()].filter((t) => t > 0).length;

  // Trend: one point per day over the last TREND_DAYS days.
  const trend: { date: Date; tokens: number }[] = [];
  for (let i = TREND_DAYS - 1; i >= 0; i--) {
    const d = new Date(today);
    d.setDate(today.getDate() - i);
    trend.push({ date: d, tokens: totals.get(localDate(d)) ?? 0 });
  }
  const peak = Math.max(0, ...trend.map((t) => t.tokens));
  const W = 300;
  const H = 80;
  const path = smoothPath(
    trend.map((t, i) => [(i / (TREND_DAYS - 1)) * W, H - 6 - (peak ? (t.tokens / peak) * (H - 12) : 0)]),
  );
  const md = (d: Date) => `${d.getMonth() + 1}/${d.getDate()}`;

  return (
    <section className="panel activity">
      <div className="row">
        <h2>Activity</h2>
        <span className="muted small">{active} active days</span>
      </div>
      <div className="heatmap" style={{ gridTemplateColumns: `repeat(${weeks}, 1fr)` }}>
        {cells.map((c) => (
          <span
            key={c.date}
            className="cell"
            data-hidden={c.hidden || undefined}
            // Square root keeps light days visible next to heavy ones.
            style={{ "--level": c.tokens ? 0.25 + 0.75 * Math.sqrt(c.tokens / max) : 0 } as React.CSSProperties}
            title={`${c.date}: ${compact(c.tokens)} tokens`}
          />
        ))}
      </div>
      <div className="axis months">
        {monthStarts.map((m) => (
          <span key={m.month} style={{ left: `${((m.week + 0.5) / weeks) * 100}%` }}>
            {MONTHS[m.month]}
          </span>
        ))}
      </div>

      <div className="row trend-head">
        <h2>Trend</h2>
        <span className="muted small">Peak {compact(peak)}</span>
      </div>
      <svg className="trend" viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" aria-label={`Daily tokens, peak ${compact(peak)}`}>
        <path d={path} />
      </svg>
      <div className="axis dates">
        <span>{md(trend[0].date)}</span>
        <span>{md(trend[Math.floor(TREND_DAYS / 2)].date)}</span>
        <span>{md(trend[TREND_DAYS - 1].date)}</span>
      </div>
    </section>
  );
}

export function ProviderMark({ provider }: { provider: Provider }) {
  return (
    <svg className="mark" viewBox="0 0 24 24" role="img" aria-label={provider === "claude" ? "Claude" : "ChatGPT"}>
      <path fillRule="evenodd" d={provider === "claude" ? CLAUDE_LOGO : OPENAI_LOGO} />
    </svg>
  );
}
