//! Tracks what each Claude Code session is doing, from the hook events it
//! posts to Mika's local listener.

use std::collections::HashMap;
use std::io::Read;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

pub const HOOK_ADDR: &str = "127.0.0.1:47321";

/// How long a finished session shows as done before it settles to idle.
const DONE_LINGER_MS: u64 = 30_000;
/// Sessions with no events for this long are dropped (crashed or killed without SessionEnd).
const STALE_MS: u64 = 6 * 60 * 60 * 1000;
const MAX_BODY: u64 = 1 << 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentState {
    // Ordered by urgency: the tray shows the most urgent session.
    Idle,
    Done,
    Thinking,
    Working,
    Waiting,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionStatus {
    pub session_id: String,
    pub cwd: Option<String>,
    pub state: AgentState,
    /// The tool being run while `state` is working.
    pub tool: Option<String>,
    pub since_ms: u64,
}

/// The fields Mika uses from a Claude Code hook payload.
#[derive(Debug, Deserialize)]
pub struct HookEvent {
    pub session_id: String,
    pub hook_event_name: String,
    pub cwd: Option<String>,
    pub tool_name: Option<String>,
}

#[derive(Default)]
pub struct Tracker {
    sessions: HashMap<String, SessionStatus>,
}

impl Tracker {
    pub fn apply(&mut self, event: HookEvent, now_ms: u64) {
        let (state, tool) = match event.hook_event_name.as_str() {
            "SessionStart" => (AgentState::Idle, None),
            "UserPromptSubmit" | "PostToolUse" => (AgentState::Thinking, None),
            "PreToolUse" => (AgentState::Working, event.tool_name),
            "Notification" => (AgentState::Waiting, None),
            "Stop" => (AgentState::Done, None),
            "SessionEnd" => {
                self.sessions.remove(&event.session_id);
                return;
            }
            _ => return,
        };
        let entry = self
            .sessions
            .entry(event.session_id.clone())
            .or_insert_with(|| SessionStatus {
                session_id: event.session_id,
                cwd: None,
                state,
                tool: None,
                since_ms: now_ms,
            });
        if event.cwd.is_some() {
            entry.cwd = event.cwd;
        }
        entry.state = state;
        entry.tool = tool;
        entry.since_ms = now_ms;
    }

    pub fn snapshot(&mut self, now_ms: u64) -> Vec<SessionStatus> {
        self.sessions
            .retain(|_, s| now_ms.saturating_sub(s.since_ms) < STALE_MS);
        let mut out: Vec<SessionStatus> = self
            .sessions
            .values()
            .cloned()
            .map(|mut s| {
                if s.state == AgentState::Done && now_ms.saturating_sub(s.since_ms) >= DONE_LINGER_MS {
                    s.state = AgentState::Idle;
                }
                s
            })
            .collect();
        out.sort_by(|a, b| a.session_id.cmp(&b.session_id));
        out
    }
}

pub fn overall(sessions: &[SessionStatus]) -> Option<AgentState> {
    sessions.iter().map(|s| s.state).max()
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Serves hook posts until the process exits, calling `on_event` for each valid one.
pub fn serve(on_event: impl Fn(HookEvent)) -> std::io::Result<()> {
    let server = tiny_http::Server::http(HOOK_ADDR).map_err(std::io::Error::other)?;
    for mut request in server.incoming_requests() {
        let status = if request.method() != &tiny_http::Method::Post || request.url() != "/hook" {
            404
        } else {
            let mut body = Vec::new();
            match request.as_reader().take(MAX_BODY).read_to_end(&mut body) {
                Ok(_) => match serde_json::from_slice::<HookEvent>(&body) {
                    Ok(event) => {
                        on_event(event);
                        204
                    }
                    Err(_) => 400,
                },
                Err(_) => 400,
            }
        };
        let _ = request.respond(tiny_http::Response::empty(status));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(name: &str, tool: Option<&str>) -> HookEvent {
        HookEvent {
            session_id: "s1".into(),
            hook_event_name: name.into(),
            cwd: Some("/tmp/project".into()),
            tool_name: tool.map(Into::into),
        }
    }

    #[test]
    fn follows_a_turn() {
        let mut t = Tracker::default();
        t.apply(event("UserPromptSubmit", None), 0);
        assert_eq!(t.snapshot(0)[0].state, AgentState::Thinking);

        t.apply(event("PreToolUse", Some("Bash")), 10);
        let s = &t.snapshot(10)[0];
        assert_eq!(s.state, AgentState::Working);
        assert_eq!(s.tool.as_deref(), Some("Bash"));

        t.apply(event("Notification", None), 20);
        assert_eq!(t.snapshot(20)[0].state, AgentState::Waiting);

        t.apply(event("Stop", None), 30);
        assert_eq!(t.snapshot(30)[0].state, AgentState::Done);
        assert_eq!(t.snapshot(30 + DONE_LINGER_MS)[0].state, AgentState::Idle);

        t.apply(event("SessionEnd", None), 40);
        assert!(t.snapshot(40).is_empty());
    }

    #[test]
    fn drops_stale_sessions_and_ranks_urgency() {
        let mut t = Tracker::default();
        t.apply(event("PreToolUse", Some("Read")), 0);
        assert!(t.snapshot(STALE_MS).is_empty());

        let sessions = vec![
            SessionStatus { session_id: "a".into(), cwd: None, state: AgentState::Working, tool: None, since_ms: 0 },
            SessionStatus { session_id: "b".into(), cwd: None, state: AgentState::Waiting, tool: None, since_ms: 0 },
        ];
        assert_eq!(overall(&sessions), Some(AgentState::Waiting));
        assert_eq!(overall(&[]), None);
    }
}
