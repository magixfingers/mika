//! Answering an agent's permission requests from Mika's popup.
//!
//! Claude Code and Codex run Mika's `PermissionRequest` hook before they show
//! their own approval prompt. The hook posts the request here and waits; Mika
//! holds the HTTP response open until the user picks Allow, Always allow or
//! Deny in the popup, then answers with the agent's decision JSON. Any other
//! outcome (Answer in terminal, a timeout, Mika not running) answers with
//! nothing, and the agent falls back to its normal prompt.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// How long Mika holds a request before handing it back to the terminal. The
/// hook's own timeout (in the hooks templates) is longer, so the fallback is
/// always Mika's answering with nothing, never a failed hook.
pub const WAIT: Duration = Duration::from_secs(9 * 60);
const MAX_DETAIL: usize = 400;

/// The fields Mika uses from a `PermissionRequest` hook payload.
#[derive(Debug, Clone, Deserialize)]
pub struct Request {
    /// Set from the URL the hook posted to, not the payload.
    #[serde(skip, default = "default_agent")]
    pub agent: &'static str,
    pub session_id: String,
    pub cwd: Option<String>,
    pub tool_name: String,
    #[serde(default)]
    pub tool_input: Value,
    /// Rules Claude Code would offer as "don't ask again"; sent back for Always allow.
    #[serde(default)]
    pub permission_suggestions: Option<Value>,
}

fn default_agent() -> &'static str {
    "claude"
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Choice {
    Allow,
    Always,
    Deny,
    /// Hand the request back to the agent's own prompt.
    Terminal,
}

struct Pending {
    request: Request,
    reply: Sender<Choice>,
}

/// Requests waiting for an answer, keyed by an increasing id.
#[derive(Default)]
pub struct Permissions {
    pending: Mutex<HashMap<u64, Pending>>,
    next_id: AtomicU64,
}

impl Permissions {
    pub fn add(&self, request: Request, reply: Sender<Choice>) -> u64 {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst) + 1;
        if let Ok(mut pending) = self.pending.lock() {
            pending.insert(id, Pending { request, reply });
        }
        id
    }

    /// Delivers the user's choice; false if the request already ended.
    pub fn answer(&self, id: u64, choice: Choice) -> bool {
        let entry = self.pending.lock().ok().and_then(|mut p| p.remove(&id));
        entry.is_some_and(|e| e.reply.send(choice).is_ok())
    }

    /// Forgets a request that ended without an answer (timed out).
    pub fn remove(&self, id: u64) {
        if let Ok(mut pending) = self.pending.lock() {
            pending.remove(&id);
        }
    }

    /// The oldest request still waiting, to show once the current one is answered.
    pub fn oldest(&self) -> Option<(u64, Request)> {
        let pending = self.pending.lock().ok()?;
        pending.iter().min_by_key(|(id, _)| **id).map(|(id, p)| (*id, p.request.clone()))
    }
}

/// The hook's response body, or None to let the agent show its own prompt.
/// Both agents take the same allow/deny shape; only Claude Code accepts
/// `updatedPermissions` (Codex rejects it), so Always allow is Claude-only.
pub fn decision_json(choice: Choice, request: &Request) -> Option<String> {
    let decision = match choice {
        Choice::Always if can_always(request) => {
            json!({ "behavior": "allow", "updatedPermissions": request.permission_suggestions })
        }
        Choice::Allow | Choice::Always => json!({ "behavior": "allow" }),
        Choice::Deny => json!({ "behavior": "deny", "message": "The user denied this from Mika." }),
        Choice::Terminal => return None,
    };
    Some(json!({ "hookSpecificOutput": { "hookEventName": "PermissionRequest", "decision": decision } }).to_string())
}

/// Whether Always allow has a rule to save.
pub fn can_always(request: &Request) -> bool {
    request.agent == "claude"
        && request.permission_suggestions.as_ref().and_then(Value::as_array).is_some_and(|r| !r.is_empty())
}

/// One line saying what the agent wants to do: the command, file or URL.
pub fn detail(request: &Request) -> String {
    let input = &request.tool_input;
    let field = |key: &str| input.get(key).and_then(Value::as_str).map(str::to_string);
    let text = ["command", "file_path", "notebook_path", "url", "pattern", "path"]
        .into_iter()
        .find_map(field)
        .unwrap_or_else(|| match input {
            Value::Null => String::new(),
            other => other.to_string(),
        });
    truncate(text.trim(), MAX_DETAIL)
}

fn truncate(text: &str, max: usize) -> String {
    match text.char_indices().nth(max) {
        Some((end, _)) => format!("{}…", &text[..end]),
        None => text.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    fn request(agent: &'static str, input: Value, suggestions: Option<Value>) -> Request {
        Request {
            agent,
            session_id: "s1".into(),
            cwd: Some("/tmp".into()),
            tool_name: "Bash".into(),
            tool_input: input,
            permission_suggestions: suggestions,
        }
    }

    fn parse(json: Option<String>) -> Value {
        serde_json::from_str(&json.unwrap()).unwrap()
    }

    #[test]
    fn reads_the_documented_claude_code_payload() {
        let body = r#"{"session_id":"abc123","transcript_path":"/t.jsonl","cwd":"/p","permission_mode":"default",
            "hook_event_name":"PermissionRequest","tool_name":"Bash",
            "tool_input":{"command":"rm -rf node_modules","description":"Remove node_modules directory"},
            "permission_suggestions":[{"type":"addRules","rules":[{"toolName":"Bash","ruleContent":"rm -rf node_modules"}],"behavior":"allow","destination":"localSettings"}]}"#;
        let r: Request = serde_json::from_str(body).unwrap();
        assert_eq!(detail(&r), "rm -rf node_modules");
        assert!(can_always(&r));
    }

    #[test]
    fn builds_claude_code_decisions() {
        let rules = json!([{ "type": "addRules", "rules": [], "behavior": "allow", "destination": "localSettings" }]);
        let r = request("claude", json!({ "command": "ls" }), Some(rules.clone()));
        let allow = parse(decision_json(Choice::Allow, &r));
        assert_eq!(allow["hookSpecificOutput"]["hookEventName"], "PermissionRequest");
        assert_eq!(allow["hookSpecificOutput"]["decision"], json!({ "behavior": "allow" }));
        let always = parse(decision_json(Choice::Always, &r));
        assert_eq!(always["hookSpecificOutput"]["decision"]["updatedPermissions"], rules);
        let deny = parse(decision_json(Choice::Deny, &r));
        assert_eq!(deny["hookSpecificOutput"]["decision"]["behavior"], "deny");
        assert!(decision_json(Choice::Terminal, &r).is_none());
    }

    #[test]
    fn never_sends_codex_saved_rules() {
        let rules = json!([{ "type": "addRules", "rules": [], "behavior": "allow", "destination": "localSettings" }]);
        let r = request("codex", json!({ "command": "ls" }), Some(rules));
        assert!(!can_always(&r));
        assert_eq!(parse(decision_json(Choice::Always, &r))["hookSpecificOutput"]["decision"], json!({ "behavior": "allow" }));
    }

    #[test]
    fn summarizes_and_truncates() {
        let r = request("claude", json!({ "file_path": "/src/a.rs", "old_string": "x" }), None);
        assert_eq!(detail(&r), "/src/a.rs");
        assert!(!can_always(&r));
        let long = request("claude", json!({ "command": "é".repeat(500) }), None);
        assert_eq!(detail(&long).chars().count(), MAX_DETAIL + 1);
    }

    #[test]
    fn answers_once_and_queues_the_rest() {
        let p = Permissions::default();
        let (tx1, rx1) = mpsc::channel();
        let (tx2, _rx2) = mpsc::channel();
        let a = p.add(request("claude", json!({}), None), tx1);
        let b = p.add(request("claude", json!({}), None), tx2);
        assert_eq!(p.oldest().map(|(id, _)| id), Some(a));
        assert!(p.answer(a, Choice::Deny));
        assert_eq!(rx1.recv().unwrap(), Choice::Deny);
        assert!(!p.answer(a, Choice::Allow));
        assert_eq!(p.oldest().map(|(id, _)| id), Some(b));
    }
}
