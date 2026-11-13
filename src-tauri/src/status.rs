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
    /// The agent finished and is waiting for the user's next message. Nothing
    /// is blocked, so it's calmer than `Waiting`.
    Turn,
    /// Blocked until the user answers (a permission prompt or a question).
    Waiting,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionStatus {
    pub session_id: String,
    /// Which agent reported it: "claude" or "codex".
    pub agent: &'static str,
    pub cwd: Option<String>,
    pub state: AgentState,
    /// The tool being run while `state` is working.
    pub tool: Option<String>,
    pub since_ms: u64,
}

/// The fields Mika uses from a Claude Code hook payload.
#[derive(Debug, Deserialize)]
pub struct HookEvent {
    /// Set from the URL the hook posted to, not the payload.
    #[serde(skip, default = "default_agent")]
    pub agent: &'static str,
    pub session_id: String,
    pub hook_event_name: String,
    pub cwd: Option<String>,
    pub tool_name: Option<String>,
    /// Claude Code's reason for a `Notification`, e.g. `permission_prompt` or `idle_prompt`.
    #[serde(default)]
    pub notification_type: Option<String>,
}

fn default_agent() -> &'static str {
    "claude"
}

#[derive(Default)]
pub struct Tracker {
    sessions: HashMap<String, SessionStatus>,
}

impl Tracker {
    pub fn apply(&mut self, event: HookEvent, now_ms: u64) {
        let (state, tool) = match event.hook_event_name.as_str() {
            "SessionStart" => (AgentState::Idle, None),
            "UserPromptSubmit" | "PostToolUse" | "PostToolUseFailure" => (AgentState::Thinking, None),
            "PreToolUse" => (AgentState::Working, event.tool_name),
            // Codex (and Mika's own permission hook) send a dedicated PermissionRequest.
            // Keep the tool name: for an approval request it says what needs approving.
            "PermissionRequest" => (AgentState::Waiting, event.tool_name),
            "Notification" => match notification_state(event.notification_type.as_deref()) {
                Some(state) => (state, event.tool_name),
                None => return,
            },
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
                agent: event.agent,
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

    pub fn get(&self, session_id: &str) -> Option<&SessionStatus> {
        self.sessions.get(session_id)
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

/// What a Claude Code notification says about the session: blocked on the
/// user, simply the user's turn, or nothing (sign-in results, quota notices…).
fn notification_state(kind: Option<&str>) -> Option<AgentState> {
    match kind {
        // Older Claude Code versions send no type; they only notified to ask.
        None | Some("permission_prompt" | "elicitation_dialog" | "elicitation_url_dialog" | "agent_needs_input") => {
            Some(AgentState::Waiting)
        }
        Some("idle_prompt") => Some(AgentState::Turn),
        Some(_) => None,
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

/// Serves hook posts until the process exits. Status hooks post to `/hook`
/// (Claude Code) and `/hook/codex`, and `on_event` gets each valid one.
/// Permission hooks post to `/permission` and `/permission/codex`; those go to
/// `on_permission` with the open request, which it must answer itself (the
/// hook is waiting on that answer).
pub fn serve(
    on_event: impl Fn(HookEvent),
    on_permission: impl Fn(crate::permission::Request, tiny_http::Request),
) -> std::io::Result<()> {
    let server = tiny_http::Server::http(HOOK_ADDR).map_err(std::io::Error::other)?;
    for mut request in server.incoming_requests() {
        let route = match request.url() {
            "/hook" => Some((false, "claude")),
            "/hook/codex" => Some((false, "codex")),
            "/permission" => Some((true, "claude")),
            "/permission/codex" => Some((true, "codex")),
            _ => None,
        };
        let Some((is_permission, agent)) = route.filter(|_| request.method() == &tiny_http::Method::Post) else {
            let _ = request.respond(tiny_http::Response::empty(404));
            continue;
        };
        let mut body = Vec::new();
        if request.as_reader().take(MAX_BODY).read_to_end(&mut body).is_err() {
            let _ = request.respond(tiny_http::Response::empty(400));
            continue;
        }
        if is_permission {
            match serde_json::from_slice::<crate::permission::Request>(&body) {
                Ok(mut ask) => {
                    ask.agent = agent;
                    on_permission(ask, request);
                }
                Err(_) => {
                    let _ = request.respond(tiny_http::Response::empty(400));
                }
            }
            continue;
        }
        let status = match serde_json::from_slice::<HookEvent>(&body) {
            Ok(mut event) => {
                event.agent = agent;
                on_event(event);
                204
            }
            Err(_) => 400,
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
            agent: "claude",
            session_id: "s1".into(),
            hook_event_name: name.into(),
            cwd: Some("/tmp/project".into()),
            tool_name: tool.map(Into::into),
            notification_type: None,
        }
    }

    #[test]
    fn tells_a_blocked_agent_from_an_idle_one() {
        let mut t = Tracker::default();
        let mut notify = |kind: &str| {
            let mut e = event("Notification", None);
            e.notification_type = Some(kind.into());
            t.apply(e, 0);
            t.snapshot(0)[0].state
        };
        assert_eq!(notify("idle_prompt"), AgentState::Turn);
        assert_eq!(notify("permission_prompt"), AgentState::Waiting);
        assert_eq!(notify("elicitation_dialog"), AgentState::Waiting);
        // Notices that don't need the user leave the state alone.
        assert_eq!(notify("auth_success"), AgentState::Waiting);
        t.apply(event("UserPromptSubmit", None), 1);
        assert_eq!(t.snapshot(1)[0].state, AgentState::Thinking);
        assert!(AgentState::Turn < AgentState::Waiting && AgentState::Turn > AgentState::Working);
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
    fn codex_permission_request_needs_you() {
        let mut t = Tracker::default();
        let mut e = event("PermissionRequest", None);
        e.agent = "codex";
        t.apply(e, 0);
        let s = &t.snapshot(0)[0];
        assert_eq!(s.state, AgentState::Waiting);
        assert_eq!(s.agent, "codex");
    }

    #[test]
    fn drops_stale_sessions_and_ranks_urgency() {
        let mut t = Tracker::default();
        t.apply(event("PreToolUse", Some("Read")), 0);
        assert!(t.snapshot(STALE_MS).is_empty());

        let sessions = vec![
            SessionStatus { session_id: "a".into(), agent: "claude", cwd: None, state: AgentState::Working, tool: None, since_ms: 0 },
            SessionStatus { session_id: "b".into(), agent: "codex", cwd: None, state: AgentState::Waiting, tool: None, since_ms: 0 },
        ];
        assert_eq!(overall(&sessions), Some(AgentState::Waiting));
        assert_eq!(overall(&[]), None);
    }
}
