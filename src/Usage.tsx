import type { DayUsage, ModelUsage, Provider, RateLimit, SessionStatus, SessionUsage } from "./types";
import { Mika, STATE_LABELS } from "./Mika";
import { compact, folderName, formatCost, localDate, timeAgo, timeUntil } from "./format";

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
  return (
    <div className="glass segmented" role="tablist" aria-label="Time range">
      {(["day", "month", "total"] as Range[]).map((r) => (
        <button key={r} role="tab" aria-selected={value === r} onClick={() => onChange(r)}>
          {r === "day" ? "Day" : r === "month" ? "Month" : "Total"}
        </button>
      ))}
    </div>
  );
}

export function Totals({ usage, range }: { usage: Map<string, ModelUsage>; range: Range }) {
  const tokens = [...usage.values()].reduce((s, u) => s + u.tokens, 0);
  const cost = [...usage.values()].reduce((s, u) => s + u.cost, 0);
  return (
    <section className="totals">
      <p className="eyebrow">{RANGE_LABELS[range]}</p>
      <p className="total-tokens">
        {tokens.toLocaleString()}
        <span className="total-compact">≈ {compact(tokens)}</span>
      </p>
      <p className="total-cost" title="Estimated from public API list prices">
        {formatCost(cost)} <span className="muted">est.</span>
      </p>
    </section>
  );
}

export function Limits({ codex }: { codex: RateLimit | null }) {
  if (!codex) return null;
  const left = Math.max(0, 100 - codex.used_percent);
  const window = codex.window_minutes >= 10080 ? "Weekly" : `${Math.round(codex.window_minutes / 60)}-hour`;
  return (
    <section className="glass panel">
      <h2>Limits</h2>
      <div className="limit">
        <div className="row">
          <span className="name">
            <ProviderMark provider="codex" /> Codex
          </span>
          {codex.plan && <span className="muted plan">{codex.plan}</span>}
        </div>
        <div className="meter" style={{ "--fill": left / 100 } as React.CSSProperties}>
          <span />
        </div>
        <div className="row small">
          <span className="muted">
            {window} · resets in {timeUntil(codex.resets_at_ms)}
          </span>
          <span>{left.toFixed(0)}% left</span>
        </div>
      </div>
    </section>
  );
}

export function Models({ usage }: { usage: Map<string, ModelUsage> }) {
  const rows = [...usage.entries()].sort((a, b) => b[1].tokens - a[1].tokens).slice(0, 6);
  const total = rows.reduce((s, [, u]) => s + u.tokens, 0) || 1;
  return (
    <section className="glass panel">
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

export function Sessions({
  sessions,
  statuses,
}: {
  sessions: SessionUsage[];
  statuses: SessionStatus[];
}) {
  return (
    <section className="glass panel sessions">
      <h2>Sessions</h2>
      <ul className="list">
        {sessions.map((s) => {
          const live = s.provider === "claude" ? statuses.find((st) => st.session_id === s.id) : undefined;
          const model = s.models.length === 1 ? s.models[0] : `${s.models.length} models`;
          const meta = [model, s.subagents > 0 && `${s.subagents} subagent${s.subagents === 1 ? "" : "s"}`, timeAgo(s.last_active_ms)]
            .filter(Boolean)
            .join(" · ");
          return (
            <li key={`${s.provider}:${s.id}`} className="session">
              {live ? <Mika state={live.state} size={22} /> : <ProviderMark provider={s.provider} />}
              <div className="session-text">
                <span className="name">{s.title ?? folderName(s.cwd) ?? s.id.slice(0, 13)}</span>
                <span className="muted small">{live ? `${STATE_LABELS[live.state]} · ${meta}` : meta}</span>
              </div>
              <span className="num">{compact(s.tokens)}</span>
            </li>
          );
        })}
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
    <section className="glass panel activity">
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
  return <span className={`mark mark-${provider}`} aria-label={provider === "claude" ? "Claude" : "Codex"} />;
}
