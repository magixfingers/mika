//! Samples running processes, finds AI coding agents, and groups each agent
//! with the processes it started.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use sysinfo::{Components, Pid, Process, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentKind {
    Claude,
    Codex,
}

/// One process in an agent's tree, flattened with its depth for display.
#[derive(Debug, Clone, Serialize)]
pub struct ProcNode {
    pub pid: u32,
    pub ppid: Option<u32>,
    pub name: String,
    pub command: String,
    pub cpu: f32,
    pub memory: u64,
    pub depth: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct Agent {
    pub pid: u32,
    pub kind: AgentKind,
    pub cwd: Option<String>,
    /// Sum over the agent and all its descendants. Per-core percent, so it can exceed 100.
    pub cpu: f32,
    /// Resident memory in bytes, summed over the tree.
    pub memory: u64,
    pub uptime_secs: u64,
    /// The app the agent runs inside (e.g. /Applications/ChatGPT.app or Terminal.app),
    /// used to bring its session to the front.
    pub host_app: Option<String>,
    pub processes: Vec<ProcNode>,
}

/// macOS thermal pressure, the signal the OS uses to throttle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Thermal {
    Normal,
    Fair,
    Serious,
    Critical,
    Unknown,
}

/// Machine-wide load.
#[derive(Debug, Clone, Serialize)]
pub struct SystemLoad {
    /// Average over all cores, 0..100.
    pub cpu: f32,
    pub memory_used: u64,
    pub memory_total: u64,
    /// Hottest CPU die sensor in °C; None where the Mac exposes no sensors.
    pub cpu_temp: Option<f32>,
    pub thermal: Thermal,
}

#[derive(Debug, Clone, Serialize)]
pub struct Snapshot {
    pub sampled_at_ms: u64,
    pub system: SystemLoad,
    pub agents: Vec<Agent>,
}

pub struct Sampler {
    sys: System,
    components: Option<Components>,
}

impl Sampler {
    pub fn new() -> Result<Self, sysinfo::Error> {
        Ok(Self { sys: System::new()?, components: Components::new_with_refreshed_list().ok() })
    }

    /// Refreshes process data and builds a snapshot. CPU figures are measured
    /// since the previous call, so the first snapshot reports zero CPU.
    pub fn sample(&mut self) -> Snapshot {
        self.sys.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing()
                .with_cpu()
                .with_memory()
                .with_exe(UpdateKind::OnlyIfNotSet)
                .with_cmd(UpdateKind::OnlyIfNotSet)
                .with_cwd(UpdateKind::OnlyIfNotSet),
        );
        self.sys.refresh_cpu_usage();
        self.sys.refresh_memory();
        let cpu_temp = self.components.as_mut().and_then(cpu_temp);
        let system = SystemLoad {
            cpu: self.sys.global_cpu_usage(),
            memory_used: self.sys.used_memory(),
            memory_total: self.sys.total_memory(),
            cpu_temp,
            thermal: thermal_state(),
        };
        build_snapshot(&self.sys, system)
    }
}

/// The hottest Apple Silicon CPU die sensor ("PMU tdieN"). The "tdev" sensors
/// next to them report nonsense (around -9200 °C), so only dies count.
fn cpu_temp(components: &mut Components) -> Option<f32> {
    components.refresh(false);
    components
        .iter()
        .filter(|c| c.label().starts_with("PMU tdie"))
        .filter_map(|c| c.temperature())
        .filter(|t| (0.0..150.0).contains(t))
        .reduce(f32::max)
}

#[cfg(target_os = "macos")]
fn thermal_state() -> Thermal {
    use objc2_foundation::{NSProcessInfo, NSProcessInfoThermalState};
    match NSProcessInfo::processInfo().thermalState() {
        NSProcessInfoThermalState::Nominal => Thermal::Normal,
        NSProcessInfoThermalState::Fair => Thermal::Fair,
        NSProcessInfoThermalState::Serious => Thermal::Serious,
        NSProcessInfoThermalState::Critical => Thermal::Critical,
        _ => Thermal::Unknown,
    }
}

#[cfg(not(target_os = "macos"))]
fn thermal_state() -> Thermal {
    Thermal::Unknown
}

fn build_snapshot(sys: &System, system: SystemLoad) -> Snapshot {
    let procs = sys.processes();

    let mut children: HashMap<Pid, Vec<Pid>> = HashMap::new();
    for (pid, p) in procs {
        if let Some(parent) = p.parent() {
            children.entry(parent).or_default().push(*pid);
        }
    }

    let kinds: HashMap<Pid, AgentKind> = procs
        .iter()
        .filter_map(|(pid, p)| agent_kind(p).map(|k| (*pid, k)))
        .collect();

    let mut agents: Vec<Agent> = kinds
        .iter()
        // An agent started by another agent (e.g. a subagent) belongs to its root's tree.
        .filter(|(pid, _)| !has_agent_ancestor(sys, **pid, &kinds))
        .filter_map(|(pid, kind)| build_agent(sys, *pid, *kind, &children))
        .collect();
    agents = merge_codex_siblings(agents);
    agents.sort_by_key(|a| a.pid);

    Snapshot {
        sampled_at_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0),
        system,
        agents,
    }
}

/// The ChatGPT app runs Codex as sibling processes (`codex app-server` plus
/// `codex exec-server`), so Codex roots with the same parent are one agent,
/// named after the process using the most memory. Claude roots stay separate:
/// the Claude app starts one process per independent session.
fn merge_codex_siblings(agents: Vec<Agent>) -> Vec<Agent> {
    let mut out: Vec<Agent> = Vec::new();
    let mut by_parent: HashMap<u32, usize> = HashMap::new();
    for agent in agents {
        let parent = agent.processes.first().and_then(|p| p.ppid);
        let slot = match (agent.kind, parent) {
            (AgentKind::Codex, Some(ppid)) => by_parent.get(&ppid).copied(),
            _ => None,
        };
        match slot {
            Some(i) => {
                let merged = &mut out[i];
                if agent.memory > merged.memory {
                    merged.pid = agent.pid;
                    merged.host_app = agent.host_app.clone();
                    merged.cwd = agent.cwd.clone();
                    merged.uptime_secs = agent.uptime_secs;
                }
                merged.cpu += agent.cpu;
                merged.memory += agent.memory;
                merged.processes.extend(agent.processes);
            }
            None => {
                if let (AgentKind::Codex, Some(ppid)) = (agent.kind, parent) {
                    by_parent.insert(ppid, out.len());
                }
                out.push(agent);
            }
        }
    }
    out
}

fn agent_kind(p: &Process) -> Option<AgentKind> {
    let exe_name = p
        .exe()
        .and_then(|e| e.file_name())
        .unwrap_or_else(|| p.name());
    match exe_name.to_str() {
        Some("claude") => return Some(AgentKind::Claude),
        Some("codex") => return Some(AgentKind::Codex),
        _ => {}
    }

    // npm installs run as `node .../@anthropic-ai/claude-code/cli.js`.
    if exe_name == OsStr::new("node") {
        let args = p.cmd();
        if args.iter().any(|a| a.to_string_lossy().contains("@anthropic-ai/claude-code")) {
            return Some(AgentKind::Claude);
        }
        if args.iter().any(|a| a.to_string_lossy().contains("@openai/codex")) {
            return Some(AgentKind::Codex);
        }
    }
    None
}

fn has_agent_ancestor(sys: &System, pid: Pid, kinds: &HashMap<Pid, AgentKind>) -> bool {
    let mut current = sys.process(pid).and_then(|p| p.parent());
    // Bounded walk guards against parent cycles from pid reuse.
    for _ in 0..64 {
        match current {
            Some(parent) if kinds.contains_key(&parent) => return true,
            Some(parent) => current = sys.process(parent).and_then(|p| p.parent()),
            None => return false,
        }
    }
    false
}

fn build_agent(
    sys: &System,
    root: Pid,
    kind: AgentKind,
    children: &HashMap<Pid, Vec<Pid>>,
) -> Option<Agent> {
    let root_proc = sys.process(root)?;
    let mut processes = Vec::new();
    collect_tree(sys, root, 0, children, &mut processes);

    Some(Agent {
        pid: root.as_u32(),
        kind,
        cwd: root_proc.cwd().map(|c| c.display().to_string()),
        cpu: processes.iter().map(|n| n.cpu).sum(),
        memory: processes.iter().map(|n| n.memory).sum(),
        uptime_secs: root_proc.run_time(),
        host_app: host_app(sys, root),
        processes,
    })
}

/// The outermost `.app` bundle in an executable path, e.g.
/// `/Applications/Claude.app/Contents/Helpers/x` → `/Applications/Claude.app`.
fn app_bundle(exe: &std::path::Path) -> Option<String> {
    let path = exe.to_str()?;
    let end = path.find(".app/")?;
    Some(path[..end + 4].to_string())
}

/// The highest ancestor process that belongs to an app: Terminal for a shell
/// session, the ChatGPT app for Codex, the Claude app for its sessions.
fn host_app(sys: &System, pid: Pid) -> Option<String> {
    let mut found = None;
    let mut current = sys.process(pid).and_then(|p| p.parent());
    for _ in 0..32 {
        let Some(id) = current else { break };
        let Some(proc_) = sys.process(id) else { break };
        if let Some(bundle) = proc_.exe().and_then(app_bundle) {
            found = Some(bundle);
        }
        current = proc_.parent();
    }
    found
}

fn collect_tree(
    sys: &System,
    pid: Pid,
    depth: usize,
    children: &HashMap<Pid, Vec<Pid>>,
    out: &mut Vec<ProcNode>,
) {
    if depth > 32 {
        return;
    }
    let Some(p) = sys.process(pid) else { return };
    out.push(ProcNode {
        pid: pid.as_u32(),
        ppid: p.parent().map(|pp| pp.as_u32()),
        name: p.name().to_string_lossy().into_owned(),
        command: short_command(p),
        cpu: p.cpu_usage(),
        memory: p.memory(),
        depth,
    });
    if let Some(kids) = children.get(&pid) {
        let mut kids = kids.clone();
        kids.sort();
        for kid in kids {
            collect_tree(sys, kid, depth + 1, children, out);
        }
    }
}

fn short_command(p: &Process) -> String {
    let joined = p
        .cmd()
        .iter()
        .map(|a| a.to_string_lossy())
        .collect::<Vec<_>>()
        .join(" ");
    const MAX: usize = 200;
    if joined.chars().count() > MAX {
        let truncated: String = joined.chars().take(MAX).collect();
        format!("{truncated}…")
    } else {
        joined
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_enclosing_claude_code_session() {
        // Claude Code sets CLAUDECODE=1 for the commands it runs; elsewhere there is nothing to find.
        if std::env::var("CLAUDECODE").is_err() {
            return;
        }
        let mut sampler = Sampler::new().expect("sampler");
        sampler.sample();
        std::thread::sleep(std::time::Duration::from_millis(500));
        let snapshot = sampler.sample();

        let me = std::process::id();
        let agent = snapshot
            .agents
            .iter()
            .find(|a| a.processes.iter().any(|p| p.pid == me))
            .expect("this test process should sit inside a detected agent tree");
        assert_eq!(agent.kind, AgentKind::Claude);
        assert!(agent.memory > 0);
        assert_eq!(agent.processes[0].pid, agent.pid);
        assert_eq!(agent.processes[0].depth, 0);
    }
}

#[cfg(test)]
mod merge_tests {
    use super::*;

    fn agent(pid: u32, ppid: u32, kind: AgentKind, memory: u64) -> Agent {
        Agent {
            pid,
            kind,
            cwd: None,
            cpu: 1.0,
            memory,
            uptime_secs: 0,
            host_app: None,
            processes: vec![ProcNode {
                pid,
                ppid: Some(ppid),
                name: "x".into(),
                command: String::new(),
                cpu: 1.0,
                memory,
                depth: 0,
            }],
        }
    }

    #[test]
    fn finds_the_outermost_app_bundle() {
        let p = |s: &str| app_bundle(std::path::Path::new(s));
        assert_eq!(p("/Applications/Claude.app/Contents/Helpers/disclaimer").as_deref(), Some("/Applications/Claude.app"));
        assert_eq!(
            p("/Applications/ChatGPT.app/Contents/Resources/codex-cli/CodexCLI.app/Contents/MacOS/codex").as_deref(),
            Some("/Applications/ChatGPT.app")
        );
        assert_eq!(p("/bin/zsh"), None);
    }

    #[test]
    fn merges_codex_siblings_but_not_claude() {
        let merged = merge_codex_siblings(vec![
            agent(10, 1, AgentKind::Codex, 50),
            agent(11, 1, AgentKind::Codex, 2000),
            agent(20, 2, AgentKind::Claude, 100),
            agent(21, 2, AgentKind::Claude, 100),
        ]);
        assert_eq!(merged.len(), 3);
        let codex = merged.iter().find(|a| a.kind == AgentKind::Codex).unwrap();
        assert_eq!(codex.pid, 11);
        assert_eq!(codex.memory, 2050);
        assert_eq!(codex.processes.len(), 2);
    }
}

#[cfg(test)]
mod load_check {
    use super::*;

    /// Prints Mika's system load for comparison with macOS tools:
    /// `cargo test --release --lib system_load -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn system_load() {
        let mut s = Sampler::new().unwrap();
        s.sample();
        std::thread::sleep(std::time::Duration::from_secs(2));
        let l = s.sample().system;
        let gb = |b: u64| b as f64 / 1024f64.powi(3);
        println!(
            "MIKA cpu={:.1}% ram={:.2}/{:.2}G ({:.0}%) cpu_temp={:?} thermal={:?}",
            l.cpu,
            gb(l.memory_used),
            gb(l.memory_total),
            l.memory_used as f64 / l.memory_total as f64 * 100.0,
            l.cpu_temp,
            l.thermal
        );
    }
}
