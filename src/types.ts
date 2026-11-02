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
