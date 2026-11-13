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
  host_app: string | null;
  processes: ProcNode[];
}

export type Thermal = "normal" | "fair" | "serious" | "critical" | "unknown";

export interface SystemLoad {
  cpu: number;
  memory_used: number;
  memory_total: number;
  /** Hottest CPU die sensor in °C; null where the Mac exposes none. */
  cpu_temp: number | null;
  thermal: Thermal;
}

export interface Snapshot {
  sampled_at_ms: number;
  system: SystemLoad;
  agents: Agent[];
}

// Mirrors src-tauri/src/status.rs.

/** "turn": finished and waiting for the next message (nothing blocked);
 *  "waiting": blocked until the user answers. */
export type AgentState = "idle" | "done" | "thinking" | "working" | "turn" | "waiting";

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
export type Skin = "classic" | "liquid";
export type Backdrop = "pastel" | "mono" | "ocean" | "sunset";

/** The Liquid Glass look: 0–100 sliders, except frost (blur in px, 0–40). */
export interface GlassLook {
  transparency: number;
  frost: number;
  refraction: number;
  shine: number;
  shadow: number;
  backdrop: Backdrop;
}

export const DEFAULT_GLASS: GlassLook = {
  transparency: 45,
  frost: 18,
  refraction: 50,
  shine: 70,
  shadow: 50,
  backdrop: "pastel",
};

export interface Settings {
  sound_enabled: boolean;
  sound: string;
  alert_on_done: boolean;
  turn_alert: "off" | "status" | "chime";
  alert_popup: boolean;
  answer_from_mika: boolean;
  quiet_hours: boolean;
  quiet_start: string;
  quiet_end: string;
  default_range: "day" | "month" | "total";
  show_cost: boolean;
  motion: Motion;
  tray_title: TrayStyle;
  skin: Skin;
  glass: GlassLook;
  track_claude: boolean;
  track_codex: boolean;
  claude_limits: boolean;
  sample_interval_ms: number;
}

// Mirrors src-tauri/src/alert.rs.

export interface AlertInfo {
  kind: "waiting" | "done" | "permission";
  session_id: string;
  agent: string;
  title: string;
  body: string;
  tool: string | null;
  host_app: string | null;
  /** The agent process, used to find its exact terminal tab. */
  pid: number | null;
  since_ms: number;
  /** Set for a permission request the popup can answer. */
  request_id: number | null;
  /** What the agent wants to run: the command, file or URL. */
  detail: string | null;
  /** Whether "Always allow" has a rule to save (Claude Code only). */
  can_always: boolean;
}

export type PermissionChoice = "allow" | "always" | "deny" | "terminal";
