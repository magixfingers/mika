// Mirrors the structs serialized by src-tauri/src/sampler.rs.

export type AgentKind = "claude" | "codex";

export interface ProcNode {
  pid: number;
  ppid: number | null;
  name: string;
  command: string;
  cpu: number;
  memory: number;
  depth: number;
}

export interface Agent {
  pid: number;
  kind: AgentKind;
  cwd: string | null;
  cpu: number;
  memory: number;
  uptime_secs: number;
  processes: ProcNode[];
}

export type Thermal = "normal" | "fair" | "serious" | "critical" | "unknown";

export interface SystemLoad {
  cpu: number;
  memory_used: number;
  memory_total: number;
  swap_used: number;
  swap_total: number;
  swapping: boolean;
  thermal: Thermal;
}

export interface Snapshot {
  sampled_at_ms: number;
  system: SystemLoad;
  agents: Agent[];
}

// Mirrors src-tauri/src/status.rs.

export type AgentState = "idle" | "done" | "thinking" | "working" | "waiting";

export interface SessionStatus {
  session_id: string;
  agent: "claude" | "codex";
  cwd: string | null;
  state: AgentState;
  tool: string | null;
  since_ms: number;
}

// Mirrors src-tauri/src/tokens.rs.

export type Provider = "claude" | "codex";

export interface ModelUsage {
  tokens: number;
  cost: number;
}

export interface DayUsage {
  date: string;
  models: Record<string, ModelUsage>;
}

export interface SessionUsage {
  id: string;
  provider: Provider;
  title: string | null;
  cwd: string | null;
  models: string[];
  effort: string | null;
  tokens: number;
  cost: number;
  calls: number;
  cache_hit: number;
  subagents: number;
  last_active_ms: number;
  context_percent: number | null;
  cache_expires_ms: number | null;
}

export interface ReviewSummary {
  runs: number;
  tokens: number;
  cost: number;
  latest_ms: number;
  latest_tokens: number;
}

export interface ByRange<T> {
  day: T;
  month: T;
  total: T;
}

export interface RateLimit {
  plan: string | null;
  used_percent: number;
  window_minutes: number;
  resets_at_ms: number;
}

export interface TokenStats {
  days: DayUsage[];
  sessions: ByRange<SessionUsage[]>;
  reviews: ByRange<ReviewSummary | null>;
  codex_limit: RateLimit | null;
  tokens_per_sec: number;
}

// Mirrors src-tauri/src/claude_limits.rs.

export interface LimitWindow {
  id: string;
  used_percent: number;
  resets_at_ms: number | null;
}

export interface ClaudeLimits {
  plan: string | null;
  windows: LimitWindow[];
}

// Mirrors src-tauri/src/settings.rs.

export type Motion = "full" | "subtle" | "off";
export type TrayStyle = "full" | "compact" | "icon";

export interface Settings {
  sound_enabled: boolean;
  sound: string;
  dock_bounce: boolean;
  alert_on_done: boolean;
  quiet_hours: boolean;
  quiet_start: string;
  quiet_end: string;
  default_range: "day" | "month" | "total";
  show_cost: boolean;
  motion: Motion;
  tray_title: TrayStyle;
  track_claude: boolean;
  track_codex: boolean;
  claude_limits: boolean;
  sample_interval_ms: number;
}
