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

export interface Snapshot {
  sampled_at_ms: number;
  agents: Agent[];
}

// Mirrors src-tauri/src/status.rs.

export type AgentState = "idle" | "done" | "thinking" | "working" | "waiting";

export interface SessionStatus {
  session_id: string;
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
  subagents: number;
  last_active_ms: number;
}

export interface RateLimit {
  plan: string | null;
  used_percent: number;
  window_minutes: number;
  resets_at_ms: number;
}

export interface TokenStats {
  days: DayUsage[];
  sessions: SessionUsage[];
  codex_limit: RateLimit | null;
  tokens_per_sec: number;
}
