//! Reads token usage from Claude Code and Codex session logs.
//!
//! The logs are large and append-only, so each file is read incrementally:
//! a scan only parses lines added since the previous scan.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
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
    /// API calls: Claude messages or Codex token events.
    pub calls: u64,
    /// Share of input served from the prompt cache, 0..1.
    pub cache_hit: f64,
    pub subagents: usize,
    pub last_active_ms: i64,
    /// Last call's input as a share of the context window; Codex only, since
    /// Claude logs don't record the window size.
    pub context_percent: Option<f64>,
    /// When Claude's prompt cache for this session expires.
    pub cache_expires_ms: Option<i64>,
}

/// Codex's automatic review runs, summarized as one row.
#[derive(Debug, Clone, Serialize)]
pub struct ReviewSummary {
    pub runs: usize,
    pub tokens: u64,
    pub cost: f64,
    pub latest_ms: i64,
    pub latest_tokens: u64,
}

/// The same list for each range the UI offers, so switching is instant.
#[derive(Debug, Clone, Serialize)]
pub struct ByRange<T> {
    pub day: T,
    pub month: T,
    pub total: T,
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
    pub sessions: ByRange<Vec<SessionUsage>>,
    pub reviews: ByRange<Option<ReviewSummary>>,
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
struct SessionDay {
    models: BTreeMap<String, Usage>,
    calls: u64,
}

#[derive(Default)]
struct Session {
    provider: Option<Provider>,
    title: Option<String>,
    cwd: Option<String>,
    /// Usage by local date, so each range can be summed.
    days: BTreeMap<String, SessionDay>,
    effort: Option<String>,
    /// Subagent id → local dates it did work, so counts follow the range.
    subagents: HashMap<String, BTreeSet<String>>,
    last_active_ms: i64,
    /// Last call's input tokens and the model's context window (Codex).
    context: Option<(u64, u64)>,
    /// Claude prompt-cache lifetime set by the latest cache write.
    cache_ttl_ms: Option<i64>,
}

/// One Codex auto-review thread (one log file).
#[derive(Default)]
struct ReviewRun {
    days: BTreeMap<String, Usage>,
    last_ms: i64,
}

/// Which local dates a range covers.
#[derive(Clone, Copy)]
enum Range {
    Day,
    Month,
    Total,
}

impl Range {
    fn contains(self, date: &str, today: &str) -> bool {
        match self {
            Range::Day => date == today,
            Range::Month => date.get(..7) == today.get(..7),
            Range::Total => true,
        }
    }
}

/// Per-file parse state that must survive between incremental reads.
#[derive(Default)]
struct FileState {
    offset: u64,
    codex_model: Option<String>,
    codex_effort: Option<String>,
    codex_session: Option<String>,
    /// Set when this file is a spawned subagent thread.
    codex_subagent_id: Option<String>,
    /// Running totals from the last token event: total, input, cached input, output.
    codex_totals: [u64; 4],
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
    reviews: HashMap<PathBuf, ReviewRun>,
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
            reviews: HashMap::new(),
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

    /// Folders and files to watch for changes.
    pub fn watch_targets(&self) -> Vec<PathBuf> {
        vec![self.claude_root.clone(), self.codex_root.clone(), self.codex_index.clone()]
    }

    /// Reads only the given changed files, routing each to its parser.
    pub fn scan_paths(&mut self, paths: impl IntoIterator<Item = PathBuf>) -> bool {
        let mut changed = false;
        for path in paths {
            if path == self.codex_index {
                changed |= self.read_new_lines(&path, |s, _, line| s.codex_index_line(line));
            } else if path.extension().is_none_or(|e| e != "jsonl") {
                continue;
            } else if path.starts_with(&self.claude_root) {
                changed |= self.read_new_lines(&path, |s, p, line| s.claude_line(p, line));
            } else if path.starts_with(&self.codex_root) {
                changed |= self.read_new_lines(&path, |s, p, line| s.codex_line(p, line));
            }
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
        // A cache write sets the lifetime; reads refresh it from the last call.
        let creation = &u["cache_creation"];
        if creation["ephemeral_1h_input_tokens"].as_u64().unwrap_or(0) > 0 {
            session.cache_ttl_ms = Some(60 * 60 * 1000);
        } else if creation["ephemeral_5m_input_tokens"].as_u64().unwrap_or(0) > 0 {
            session.cache_ttl_ms = Some(5 * 60 * 1000);
        }
        if path.components().any(|c| c.as_os_str() == "subagents") {
            session.subagents.entry(path.display().to_string()).or_default().insert(local_date(ts));
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
                // A subagent's file later repeats its parent's session_meta; only the first describes this thread.
                if self.files.get(path).is_some_and(|f| f.codex_session.is_some()) {
                    return;
                }
                let Some(id) = p["id"].as_str() else { return };
                // Subagent threads roll up into the thread that started them.
                let root = p["parent_thread_id"].as_str().unwrap_or(id);
                let key = format!("codex:{root}");
                let session = self.session(&key, Provider::Codex);
                if session.cwd.is_none() {
                    session.cwd = p["cwd"].as_str().map(String::from);
                }
                let state = self.files.entry(path.to_path_buf()).or_default();
                state.codex_session = Some(key);
                // Spawned subagents count; review ("guardian") threads also hang off the parent but don't.
                let spawned = root != id && p["source"]["subagent"]["thread_spawn"].is_object();
                state.codex_subagent_id = spawned.then(|| id.to_string());
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
        let running = &info["total_token_usage"];
        let field = |v: &Value, k: &str| v[k].as_u64().unwrap_or(0);
        let totals = [
            field(running, "total_tokens"),
            field(running, "input_tokens"),
            field(running, "cached_input_tokens"),
            field(running, "output_tokens"),
        ];
        let state = self.files.entry(path.to_path_buf()).or_default();
        // Codex re-emits the running total without new usage; skip repeats.
        if info.is_null() || totals[0] == state.codex_totals[0] {
            return;
        }
        let previous = std::mem::replace(&mut state.codex_totals, totals);
        let Some(key) = state.codex_session.clone() else { return };
        let model = state.codex_model.clone().unwrap_or_else(|| "codex".into());
        let effort = state.codex_effort.clone();
        let subagent_id = state.codex_subagent_id.clone();

        let last = &info["last_token_usage"];
        let input = field(last, "input_tokens");
        // Usage is the growth of the running totals: Codex sometimes skips a
        // per-call event, and summing last_token_usage would lose those calls.
        // If the totals went backwards (a reset), fall back to the last call.
        let grew = totals.iter().zip(&previous).all(|(now, before)| now >= before);
        let [d_input, d_cached, d_output] = if grew {
            [totals[1] - previous[1], totals[2] - previous[2], totals[3] - previous[3]]
        } else {
            [input, field(last, "cached_input_tokens"), field(last, "output_tokens")]
        };
        let usage = Usage {
            // OpenAI counts cached tokens inside input_tokens.
            input: d_input.saturating_sub(d_cached),
            output: d_output,
            cache_read: d_cached,
            cache_write: 0,
        };
        if model == REVIEW_MODEL {
            let run = self.reviews.entry(path.to_path_buf()).or_default();
            run.days.entry(local_date(ts)).or_default().add(&usage);
            run.last_ms = run.last_ms.max(ts);
        } else {
            let window = info["model_context_window"].as_u64().unwrap_or(0);
            let session = self.session(&key, Provider::Codex);
            if let Some(effort) = effort {
                session.effort = Some(effort);
            }
            if let Some(id) = subagent_id {
                session.subagents.entry(id).or_default().insert(local_date(ts));
            } else if window > 0 {
                // The context bar describes the main thread, not its subagents.
                session.context = Some((input, window));
            }

        }
        self.record(&key, &model, usage, ts);
    }

    fn session(&mut self, key: &str, provider: Provider) -> &mut Session {
        let s = self.sessions.entry(key.to_string()).or_default();
        s.provider.get_or_insert(provider);
        s
    }

    fn record(&mut self, key: &str, model: &str, usage: Usage, ts_ms: i64) {
        let day = local_date(ts_ms);
        if model != REVIEW_MODEL {
            let session = self.sessions.entry(key.to_string()).or_default();
            let session_day = session.days.entry(day.clone()).or_default();
            session_day.models.entry(model.to_string()).or_default().add(&usage);
            session_day.calls += 1;
            session.last_active_ms = session.last_active_ms.max(ts_ms);
        }
        self.days.entry(day).or_default().entry(model.to_string()).or_default().add(&usage);
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

        let today = local_date(now_ms());
        let sessions = ByRange {
            day: self.sessions_in(Range::Day, &today),
            month: self.sessions_in(Range::Month, &today),
            total: self.sessions_in(Range::Total, &today),
        };
        let reviews = ByRange {
            day: self.reviews_in(Range::Day, &today),
            month: self.reviews_in(Range::Month, &today),
            total: self.reviews_in(Range::Total, &today),
        };

        TokenStats {
            days,
            sessions,
            reviews,
            codex_limit: self.codex_limit.as_ref().map(|(_, l)| l.clone()),
            tokens_per_sec: recent as f64 / (RATE_WINDOW_MS as f64 / 1000.0),
        }
    }

    fn sessions_in(&self, range: Range, today: &str) -> Vec<SessionUsage> {
        let now = now_ms();
        let mut out: Vec<SessionUsage> = self
            .sessions
            .iter()
            .filter_map(|(key, s)| {
                let mut models: BTreeMap<&str, Usage> = BTreeMap::new();
                let mut calls = 0;
                for (_, day) in s.days.iter().filter(|(d, _)| range.contains(d, today)) {
                    calls += day.calls;
                    for (m, u) in &day.models {
                        models.entry(m.as_str()).or_default().add(u);
                    }
                }
                let usage = models.values().fold(Usage::default(), |mut acc, u| {
                    acc.add(u);
                    acc
                });
                if usage.total() == 0 {
                    return None;
                }
                let input_side = usage.input + usage.cache_read + usage.cache_write;
                let id = key.split_once(':').map(|(_, id)| id).unwrap_or(key);
                Some(SessionUsage {
                    id: id.to_string(),
                    provider: s.provider.unwrap_or(Provider::Claude),
                    title: s.title.clone().or_else(|| self.codex_titles.get(id).cloned()),
                    cwd: s.cwd.clone(),
                    models: models.keys().map(|m| m.to_string()).collect(),
                    effort: s.effort.clone(),
                    tokens: usage.total(),
                    cost: models.iter().map(|(m, u)| cost(m, u)).sum(),
                    calls,
                    cache_hit: if input_side > 0 { usage.cache_read as f64 / input_side as f64 } else { 0.0 },
                    subagents: s
                        .subagents
                        .values()
                        .filter(|days| days.iter().any(|d| range.contains(d, today)))
                        .count(),
                    last_active_ms: s.last_active_ms,
                    context_percent: s.context.map(|(used, window)| used as f64 / window as f64 * 100.0),
                    cache_expires_ms: s
                        .cache_ttl_ms
                        .map(|ttl| s.last_active_ms + ttl)
                        .filter(|expires| *expires > now),
                })
            })
            .collect();
        out.sort_by_key(|s| std::cmp::Reverse(s.last_active_ms));
        out.truncate(MAX_SESSIONS);
        out
    }

    fn reviews_in(&self, range: Range, today: &str) -> Option<ReviewSummary> {
        let mut summary = ReviewSummary { runs: 0, tokens: 0, cost: 0.0, latest_ms: 0, latest_tokens: 0 };
        for run in self.reviews.values() {
            let usage = run
                .days
                .iter()
                .filter(|(d, _)| range.contains(d, today))
                .fold(Usage::default(), |mut acc, (_, u)| {
                    acc.add(u);
                    acc
                });
            if usage.total() == 0 {
                continue;
            }
            summary.runs += 1;
            summary.tokens += usage.total();
            summary.cost += cost(REVIEW_MODEL, &usage);
            if run.last_ms > summary.latest_ms {
                summary.latest_ms = run.last_ms;
                summary.latest_tokens = usage.total();
            }
        }
        (summary.runs > 0).then_some(summary)
    }
}

fn timestamp_ms(v: &Value) -> Option<i64> {
    DateTime::parse_from_rfc3339(v.as_str()?).ok().map(|d| d.timestamp_millis())
}

fn local_date(ts_ms: i64) -> String {
    DateTime::from_timestamp_millis(ts_ms)
        .map(|d| d.with_timezone(&Local).format("%Y-%m-%d").to_string())
        .unwrap_or_default()
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
        // Running totals for `calls` calls of 100 input (60 cached) and 5 output tokens.
        let count = |calls: u64| {
            let (total, input, cached, output) = (105 * calls, 100 * calls, 60 * calls, 5 * calls);
            format!(
                r#"{{"timestamp":"2026-11-03T02:00:00Z","type":"event_msg","payload":{{"type":"token_count","info":{{"total_token_usage":{{"total_tokens":{total},"input_tokens":{input},"cached_input_tokens":{cached},"output_tokens":{output}}},"last_token_usage":{{"input_tokens":100,"cached_input_tokens":60,"output_tokens":5}}}},"rate_limits":{{"plan_type":"pro","primary":{{"used_percent":23.0,"window_minutes":10080,"resets_at":1793776467}}}}}}}}"#
            )
        };
        write(
            &codex,
            &[
                r#"{"type":"session_meta","payload":{"id":"c1","cwd":"/q"}}"#,
                r#"{"type":"turn_context","payload":{"model":"gpt-6.1-sol"}}"#,
                &count(1),
                &count(1),
            ],
        );

        let mut scanner = TokenScanner::with_roots(dir.join("claude"), dir.join("codex"), index);
        assert!(scanner.scan());
        let stats = scanner.stats();

        let claude_s = stats.sessions.total.iter().find(|s| s.id == "s1").unwrap();
        assert_eq!(claude_s.tokens, 100);
        assert_eq!(claude_s.title.as_deref(), Some("Fix bug"));
        let codex_s = stats.sessions.total.iter().find(|s| s.id == "c1").unwrap();
        assert_eq!(codex_s.tokens, 105);
        assert_eq!(codex_s.title.as_deref(), Some("Codex task"));
        assert_eq!(codex_s.models, vec!["gpt-6.1-sol"]);
        let limit = stats.codex_limit.unwrap();
        assert_eq!(limit.plan.as_deref(), Some("pro"));
        assert_eq!(limit.used_percent, 23.0);

        // Nothing new: no change. New line: only it is read.
        assert!(!scanner.scan());
        write(&codex, &[&count(2)]);
        assert!(scanner.scan());
        let codex_s = scanner.stats().sessions.total.into_iter().find(|s| s.id == "c1").unwrap();
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
        for s in &stats.sessions.day {
            println!(
                "{:?} {:?} {} {:?} calls={} cache={:.0}% ctx={:?} cache_exp={:?} sub={}",
                s.provider, s.title, s.tokens, s.models, s.calls, s.cache_hit * 100.0, s.context_percent.map(|c| c.round()), s.cache_expires_ms.map(|e| (e - now_ms()) / 60000), s.subagents
            );
        }
        println!("reviews today: {:?}", stats.reviews.day);
        println!("{:?}", stats.codex_limit);
    }

    #[test]
    fn prices_by_family() {
        let u = Usage { input: 1_000_000, output: 1_000_000, cache_read: 0, cache_write: 0 };
        assert_eq!(cost("claude-opus-5-5", &u), 30.0);
        assert_eq!(cost("claude-haiku-4-5", &u), 6.0);
    }
}

#[cfg(test)]
mod subagent_tests {
    use super::*;

    #[test]
    fn counts_working_codex_subagents_only() {
        let dir = std::env::temp_dir().join(format!("mika-sub-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let day = dir.join("codex/2026/11/03");
        std::fs::create_dir_all(&day).unwrap();
        let ts = "2026-11-03T01:00:00Z";
        let write = |name: &str, lines: &[String]| std::fs::write(day.join(name), lines.join("\n") + "\n").unwrap();
        let count = format!(r#"{{"timestamp":"{ts}","type":"event_msg","payload":{{"type":"token_count","info":{{"total_token_usage":{{"total_tokens":10,"input_tokens":10,"output_tokens":0}},"last_token_usage":{{"input_tokens":10,"output_tokens":0}}}}}}}}"#);
        write("parent.jsonl", &[
            format!(r#"{{"timestamp":"{ts}","type":"session_meta","payload":{{"id":"p"}}}}"#),
            format!(r#"{{"timestamp":"{ts}","type":"turn_context","payload":{{"model":"gpt-6.1-terra"}}}}"#),
            count.clone(),
        ]);
        write("child.jsonl", &[
            format!(r#"{{"timestamp":"{ts}","type":"session_meta","payload":{{"id":"c","parent_thread_id":"p","source":{{"subagent":{{"thread_spawn":{{}}}}}}}}}}"#),
            format!(r#"{{"timestamp":"{ts}","type":"turn_context","payload":{{"model":"gpt-6.1-terra"}}}}"#),
            count.clone(),
        ]);
        write("review.jsonl", &[
            format!(r#"{{"timestamp":"{ts}","type":"session_meta","payload":{{"id":"r","parent_thread_id":"p","source":{{"subagent":{{"other":"guardian"}}}}}}}}"#),
            format!(r#"{{"timestamp":"{ts}","type":"turn_context","payload":{{"model":"codex-auto-review"}}}}"#),
        ]);
        let mut scanner = TokenScanner::with_roots(dir.join("none"), dir.join("codex"), dir.join("index.jsonl"));
        scanner.scan();
        let s = scanner.stats().sessions.total.into_iter().find(|s| s.id == "p").unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(s.subagents, 1);
    }
}

#[cfg(test)]
mod provider_totals {
    use super::*;

    /// All-time tokens per provider from this machine's logs:
    /// `cargo test --release --lib provider_totals -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn provider_totals() {
        let mut scanner = TokenScanner::new(&PathBuf::from(std::env::var("HOME").unwrap()));
        scanner.scan();
        let (mut claude, mut codex) = (0u64, 0u64);
        for models in scanner.days.values() {
            for (m, u) in models {
                if m.starts_with("claude") { claude += u.total() } else { codex += u.total() }
            }
        }
        println!("MIKA claude={claude} codex={codex}");
    }
}
