//! Reads token usage from Claude Code and Codex session logs.
//!
//! The logs are large and append-only, so each file is read incrementally:
//! a scan only parses lines added since the previous scan.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::fs::File;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Local};
use serde::Serialize;
use serde_json::Value;

/// Window for the tokens-per-second rate.
const RATE_WINDOW_MS: i64 = 60_000;
const MAX_SESSIONS: usize = 15;
const MAX_WALK_DEPTH: usize = 6;
/// Codex's automatic review runs: counted per model and day, but not as session work.
const REVIEW_MODEL: &str = "codex-auto-review";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Claude,
    Codex,
}

/// Token counts in one usage record or one aggregate.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
pub struct Usage {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
}

impl Usage {
    pub fn total(&self) -> u64 {
        self.input + self.output + self.cache_read + self.cache_write
    }

    fn add(&mut self, other: &Usage) {
        self.input += other.input;
        self.output += other.output;
        self.cache_read += other.cache_read;
        self.cache_write += other.cache_write;
    }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ModelUsage {
    pub tokens: u64,
    pub cost: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct DayUsage {
    /// Local date, `YYYY-MM-DD`.
    pub date: String,
    pub models: BTreeMap<String, ModelUsage>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionUsage {
    pub id: String,
    pub provider: Provider,
    pub title: Option<String>,
    pub cwd: Option<String>,
    pub models: Vec<String>,
    pub effort: Option<String>,
    pub tokens: u64,
    pub cost: f64,
    pub subagents: usize,
    pub last_active_ms: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct RateLimit {
    pub plan: Option<String>,
    pub used_percent: f64,
    pub window_minutes: u64,
    pub resets_at_ms: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct TokenStats {
    pub days: Vec<DayUsage>,
    pub sessions: Vec<SessionUsage>,
    pub codex_limit: Option<RateLimit>,
    pub tokens_per_sec: f64,
}

/// USD per million tokens: input, output, cache read, cache write.
/// Estimates from public list prices; unknown models fall back by family.
fn price(model: &str) -> [f64; 4] {
    let m = model.to_ascii_lowercase();
    if m.contains("opus") {
        [5.0, 25.0, 0.5, 6.25]
    } else if m.contains("sonnet") {
        [3.0, 15.0, 0.3, 3.75]
    } else if m.contains("haiku") {
        [1.0, 5.0, 0.1, 1.25]
    } else if m.contains("mini") {
        [0.25, 2.0, 0.025, 0.0]
    } else {
        // GPT and Codex models.
        [1.25, 10.0, 0.125, 0.0]
    }
}

pub fn cost(model: &str, u: &Usage) -> f64 {
    let p = price(model);
    (u.input as f64 * p[0] + u.output as f64 * p[1] + u.cache_read as f64 * p[2] + u.cache_write as f64 * p[3])
        / 1_000_000.0
}

#[derive(Default)]
struct Session {
    provider: Option<Provider>,
    title: Option<String>,
    cwd: Option<String>,
    models: BTreeMap<String, Usage>,
    effort: Option<String>,
    subagents: HashSet<String>,
    last_active_ms: i64,
}

/// Per-file parse state that must survive between incremental reads.
#[derive(Default)]
struct FileState {
    offset: u64,
    codex_model: Option<String>,
    codex_effort: Option<String>,
    codex_session: Option<String>,
    codex_last_total: u64,
}

pub struct TokenScanner {
    claude_root: PathBuf,
    codex_root: PathBuf,
    codex_index: PathBuf,
    files: HashMap<PathBuf, FileState>,
    /// Claude streams several log lines per API message; count each message once.
    seen_messages: HashSet<String>,
    sessions: HashMap<String, Session>,
    codex_titles: HashMap<String, String>,
    days: BTreeMap<String, BTreeMap<String, Usage>>,
    recent: VecDeque<(i64, u64)>,
    /// The newest Codex rate limit seen, with the time it was logged.
    codex_limit: Option<(i64, RateLimit)>,
}

impl TokenScanner {
    pub fn new(home: &Path) -> Self {
        Self::with_roots(
            home.join(".claude/projects"),
            home.join(".codex/sessions"),
            home.join(".codex/session_index.jsonl"),
        )
    }

    fn with_roots(claude_root: PathBuf, codex_root: PathBuf, codex_index: PathBuf) -> Self {
        Self {
            claude_root,
            codex_root,
            codex_index,
            files: HashMap::new(),
            seen_messages: HashSet::new(),
            sessions: HashMap::new(),
            codex_titles: HashMap::new(),
            days: BTreeMap::new(),
            recent: VecDeque::new(),
            codex_limit: None,
        }
    }

    /// Reads new log lines and returns whether anything changed.
    pub fn scan(&mut self) -> bool {
        let mut changed = false;
        let index = self.codex_index.clone();
        changed |= self.read_new_lines(&index, |s, _, line| s.codex_index_line(line));
        for path in jsonl_files(&self.claude_root.clone()) {
            changed |= self.read_new_lines(&path, |s, p, line| s.claude_line(p, line));
        }
        for path in jsonl_files(&self.codex_root.clone()) {
            changed |= self.read_new_lines(&path, |s, p, line| s.codex_line(p, line));
        }
        changed
    }

    fn read_new_lines(&mut self, path: &Path, mut handle: impl FnMut(&mut Self, &Path, &str)) -> bool {
        let Ok(len) = path.metadata().map(|m| m.len()) else { return false };
        let mut offset = self.files.get(path).map(|f| f.offset).unwrap_or(0);
        if len < offset {
            // Truncated or replaced; start over.
            offset = 0;
        }
        if len == offset {
            return false;
        }
        let Ok(mut file) = File::open(path) else { return false };
        if file.seek(SeekFrom::Start(offset)).is_err() {
            return false;
        }
        let mut reader = BufReader::new(file);
        let mut buf = Vec::new();
        loop {
            buf.clear();
            match reader.read_until(b'\n', &mut buf) {
                Ok(0) | Err(_) => break,
                // A line without a newline is still being written; read it next time.
                Ok(_) if buf.last() != Some(&b'\n') => break,
                Ok(n) => {
                    offset += n as u64;
                    if let Ok(line) = std::str::from_utf8(&buf) {
                        handle(self, path, line);
                    }
                }
            }
        }
        self.files.entry(path.to_path_buf()).or_default().offset = offset;
        true
    }

    fn codex_index_line(&mut self, line: &str) {
        let Ok(v) = serde_json::from_str::<Value>(line) else { return };
        if let (Some(id), Some(name)) = (v["id"].as_str(), v["thread_name"].as_str()) {
            self.codex_titles.insert(id.to_string(), name.to_string());
        }
    }

    fn claude_line(&mut self, path: &Path, line: &str) {
        // Cheap filters before parsing: most lines carry neither usage nor a title.
        let is_usage = line.contains("\"usage\"") && line.contains("\"assistant\"");
        let is_title = line.contains("\"custom-title\"");
        if !is_usage && !is_title {
            return;
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else { return };
        let Some(session_id) = v["sessionId"].as_str() else { return };
        let key = format!("claude:{session_id}");

        if v["type"] == "custom-title" {
            if let Some(title) = v["customTitle"].as_str() {
                self.session(&key, Provider::Claude).title = Some(title.to_string());
            }
            return;
        }
        if v["type"] != "assistant" {
            return;
        }
        let msg = &v["message"];
        let Some(model) = msg["model"].as_str().filter(|m| !m.starts_with('<')) else { return };
        if let Some(id) = msg["id"].as_str()
            && !self.seen_messages.insert(id.to_string())
        {
            return;
        }
        let u = &msg["usage"];
        let usage = Usage {
            input: u["input_tokens"].as_u64().unwrap_or(0),
            output: u["output_tokens"].as_u64().unwrap_or(0),
            cache_read: u["cache_read_input_tokens"].as_u64().unwrap_or(0),
            cache_write: u["cache_creation_input_tokens"].as_u64().unwrap_or(0),
        };
        let Some(ts) = timestamp_ms(&v["timestamp"]) else { return };

        let session = self.session(&key, Provider::Claude);
        if session.cwd.is_none() {
            session.cwd = v["cwd"].as_str().map(String::from);
        }
        if let Some(effort) = v["effort"].as_str() {
            session.effort = Some(effort.to_string());
        }
        if path.components().any(|c| c.as_os_str() == "subagents") {
            session.subagents.insert(path.display().to_string());
        }
        self.record(&key, model, usage, ts);
    }

    fn codex_line(&mut self, path: &Path, line: &str) {
        let interesting = line.contains("\"token_count\"")
            || line.contains("\"turn_context\"")
            || line.contains("\"session_meta\"");
        if !interesting {
            return;
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else { return };
        let p = &v["payload"];
        match (v["type"].as_str(), p["type"].as_str()) {
            (Some("session_meta"), _) => {
                let Some(id) = p["id"].as_str() else { return };
                // Subagent threads roll up into the thread that started them.
                let root = p["parent_thread_id"].as_str().unwrap_or(id);
                let key = format!("codex:{root}");
                let session = self.session(&key, Provider::Codex);
                if root != id {
                    session.subagents.insert(id.to_string());
                }
                if session.cwd.is_none() {
                    session.cwd = p["cwd"].as_str().map(String::from);
                }
                self.files.entry(path.to_path_buf()).or_default().codex_session = Some(key);
            }
            (Some("turn_context"), _) => {
                let state = self.files.entry(path.to_path_buf()).or_default();
                if let Some(model) = p["model"].as_str() {
                    state.codex_model = Some(model.to_string());
                }
                state.codex_effort = p["effort"].as_str().map(String::from);
            }
            (Some("event_msg"), Some("token_count")) => self.codex_token_count(path, &v),
            _ => {}
        }
    }

    fn codex_token_count(&mut self, path: &Path, v: &Value) {
        let p = &v["payload"];
        let Some(ts) = timestamp_ms(&v["timestamp"]) else { return };

        let limits = &p["rate_limits"];
        let primary = &limits["primary"];
        let newest = self.codex_limit.as_ref().is_none_or(|(seen, _)| ts >= *seen);
        if primary.is_object() && newest {
            let limit = RateLimit {
                // Some events omit the plan; keep the last one seen.
                plan: limits["plan_type"]
                    .as_str()
                    .map(String::from)
                    .or_else(|| self.codex_limit.as_ref().and_then(|(_, l)| l.plan.clone())),
                used_percent: primary["used_percent"].as_f64().unwrap_or(0.0),
                window_minutes: primary["window_minutes"].as_u64().unwrap_or(0),
                resets_at_ms: primary["resets_at"].as_i64().map(|s| s * 1000).unwrap_or(ts),
            };
            self.codex_limit = Some((ts, limit));
        }

        let info = &p["info"];
        let total = info["total_token_usage"]["total_tokens"].as_u64().unwrap_or(0);
        let state = self.files.entry(path.to_path_buf()).or_default();
        // Codex re-emits the running total without new usage; skip repeats.
        if info.is_null() || total == state.codex_last_total {
            return;
        }
        state.codex_last_total = total;
        let Some(key) = state.codex_session.clone() else { return };
        let model = state.codex_model.clone().unwrap_or_else(|| "codex".into());
        let effort = state.codex_effort.clone();

        let last = &info["last_token_usage"];
        let input = last["input_tokens"].as_u64().unwrap_or(0);
        let cached = last["cached_input_tokens"].as_u64().unwrap_or(0);
        let usage = Usage {
            // OpenAI counts cached tokens inside input_tokens.
            input: input.saturating_sub(cached),
            output: last["output_tokens"].as_u64().unwrap_or(0),
            cache_read: cached,
            cache_write: 0,
        };
        if model != REVIEW_MODEL
            && let Some(effort) = effort
        {
            self.session(&key, Provider::Codex).effort = Some(effort);
        }
        self.record(&key, &model, usage, ts);
    }

    fn session(&mut self, key: &str, provider: Provider) -> &mut Session {
        let s = self.sessions.entry(key.to_string()).or_default();
        s.provider.get_or_insert(provider);
        s
    }

    fn record(&mut self, key: &str, model: &str, usage: Usage, ts_ms: i64) {
        if model != REVIEW_MODEL {
            let session = self.sessions.entry(key.to_string()).or_default();
            session.models.entry(model.to_string()).or_default().add(&usage);
            session.last_active_ms = session.last_active_ms.max(ts_ms);
        }

        if let Some(date) = DateTime::from_timestamp_millis(ts_ms) {
            let day = date.with_timezone(&Local).format("%Y-%m-%d").to_string();
            self.days.entry(day).or_default().entry(model.to_string()).or_default().add(&usage);
        }
        if ts_ms > now_ms() - RATE_WINDOW_MS {
            self.recent.push_back((ts_ms, usage.total()));
        }
    }

    pub fn stats(&mut self) -> TokenStats {
        let cutoff = now_ms() - RATE_WINDOW_MS;
        self.recent.retain(|(ts, _)| *ts >= cutoff);
        let recent: u64 = self.recent.iter().map(|(_, t)| t).sum();

        let days = self
            .days
            .iter()
            .map(|(date, models)| DayUsage {
                date: date.clone(),
                models: models
                    .iter()
                    .map(|(m, u)| (m.clone(), ModelUsage { tokens: u.total(), cost: cost(m, u) }))
                    .collect(),
            })
            .collect();

        let mut sessions: Vec<SessionUsage> = self
            .sessions
            .iter()
            .filter(|(_, s)| s.models.values().any(|u| u.total() > 0))
            .map(|(key, s)| {
                let id = key.split_once(':').map(|(_, id)| id).unwrap_or(key);
                SessionUsage {
                    id: id.to_string(),
                    provider: s.provider.unwrap_or(Provider::Claude),
                    title: s.title.clone().or_else(|| self.codex_titles.get(id).cloned()),
                    cwd: s.cwd.clone(),
                    models: s.models.keys().cloned().collect(),
                    effort: s.effort.clone(),
                    tokens: s.models.values().map(Usage::total).sum(),
                    cost: s.models.iter().map(|(m, u)| cost(m, u)).sum(),
                    subagents: s.subagents.len(),
                    last_active_ms: s.last_active_ms,
                }
            })
            .collect();
        sessions.sort_by_key(|s| std::cmp::Reverse(s.last_active_ms));
        sessions.truncate(MAX_SESSIONS);

        TokenStats {
            days,
            sessions,
            codex_limit: self.codex_limit.as_ref().map(|(_, l)| l.clone()),
            tokens_per_sec: recent as f64 / (RATE_WINDOW_MS as f64 / 1000.0),
        }
    }
}

fn timestamp_ms(v: &Value) -> Option<i64> {
    DateTime::parse_from_rfc3339(v.as_str()?).ok().map(|d| d.timestamp_millis())
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn jsonl_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    walk(root, 0, &mut out);
    out
}

fn walk(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if depth > MAX_WALK_DEPTH {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        match entry.file_type() {
            Ok(t) if t.is_dir() => walk(&path, depth + 1, out),
            Ok(t) if t.is_file() && path.extension().is_some_and(|e| e == "jsonl") => out.push(path),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write(path: &Path, lines: &[&str]) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut f = std::fs::OpenOptions::new().create(true).append(true).open(path).unwrap();
        for l in lines {
            writeln!(f, "{l}").unwrap();
        }
    }

    #[test]
    fn parses_both_providers_incrementally() {
        let dir = std::env::temp_dir().join(format!("mika-tokens-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let claude = dir.join("claude/proj/s1.jsonl");
        let codex = dir.join("codex/2026/11/03/rollout-a.jsonl");
        let index = dir.join("index.jsonl");

        let assistant = r#"{"type":"assistant","sessionId":"s1","cwd":"/p","timestamp":"2026-11-03T01:00:00Z","message":{"id":"m1","model":"claude-opus-5-5","usage":{"input_tokens":10,"output_tokens":20,"cache_read_input_tokens":30,"cache_creation_input_tokens":40}}}"#;
        // The same message streamed twice must count once.
        write(&claude, &[assistant, assistant, r#"{"type":"custom-title","customTitle":"Fix bug","sessionId":"s1"}"#]);
        write(&index, &[r#"{"id":"c1","thread_name":"Codex task"}"#]);
        let count = |total: u64| {
            format!(
                r#"{{"timestamp":"2026-11-03T02:00:00Z","type":"event_msg","payload":{{"type":"token_count","info":{{"total_token_usage":{{"total_tokens":{total}}},"last_token_usage":{{"input_tokens":100,"cached_input_tokens":60,"output_tokens":5}}}},"rate_limits":{{"plan_type":"pro","primary":{{"used_percent":23.0,"window_minutes":10080,"resets_at":1793776467}}}}}}}}"#
            )
        };
        write(
            &codex,
            &[
                r#"{"type":"session_meta","payload":{"id":"c1","cwd":"/q"}}"#,
                r#"{"type":"turn_context","payload":{"model":"gpt-6.1-sol"}}"#,
                &count(105),
                &count(105),
            ],
        );

        let mut scanner = TokenScanner::with_roots(dir.join("claude"), dir.join("codex"), index);
        assert!(scanner.scan());
        let stats = scanner.stats();

        let claude_s = stats.sessions.iter().find(|s| s.id == "s1").unwrap();
        assert_eq!(claude_s.tokens, 100);
        assert_eq!(claude_s.title.as_deref(), Some("Fix bug"));
        let codex_s = stats.sessions.iter().find(|s| s.id == "c1").unwrap();
        assert_eq!(codex_s.tokens, 105);
        assert_eq!(codex_s.title.as_deref(), Some("Codex task"));
        assert_eq!(codex_s.models, vec!["gpt-6.1-sol"]);
        let limit = stats.codex_limit.unwrap();
        assert_eq!(limit.plan.as_deref(), Some("pro"));
        assert_eq!(limit.used_percent, 23.0);

        // Nothing new: no change. New line: only it is read.
        assert!(!scanner.scan());
        write(&codex, &[&count(210)]);
        assert!(scanner.scan());
        let codex_s = scanner.stats().sessions.into_iter().find(|s| s.id == "c1").unwrap();
        assert_eq!(codex_s.tokens, 210);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Scans this machine's real logs: `cargo test --release real_logs -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn real_logs() {
        let home = PathBuf::from(std::env::var("HOME").unwrap());
        let start = std::time::Instant::now();
        let mut scanner = TokenScanner::new(&home);
        scanner.scan();
        let first = start.elapsed();
        scanner.scan();
        let stats = scanner.stats();
        let total: u64 = stats.days.iter().flat_map(|d| d.models.values()).map(|m| m.tokens).sum();
        println!("first scan {first:?}, rescan {:?}, {total} tokens", start.elapsed() - first);
        for s in &stats.sessions {
            println!("{:?} {:?} {} {:?}", s.provider, s.title, s.tokens, s.models);
        }
        println!("{:?}", stats.codex_limit);
    }

    #[test]
    fn prices_by_family() {
        let u = Usage { input: 1_000_000, output: 1_000_000, cache_read: 0, cache_write: 0 };
        assert_eq!(cost("claude-opus-5-5", &u), 30.0);
        assert_eq!(cost("claude-haiku-4-5", &u), 6.0);
    }
}
