mod alert;
mod claude_limits;
mod permission;
mod sampler;
mod settings;
mod status;
mod tokens;

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use notify::{RecursiveMode, Watcher};

use alert::{AlertInfo, AlertKind, AlertState};
use claude_limits::ClaudeLimits;
use permission::{Choice, Permissions};
use sampler::{AgentKind, Sampler, Snapshot};
use settings::{Integration, Settings};
use status::{AgentState, HookEvent, SessionStatus, Tracker};
use tokens::{TokenScanner, TokenStats};
use tauri::menu::{AboutMetadata, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, State};

/// Longest wait between token updates when nothing changes (keeps tok/s decaying).
const TOKEN_TICK: Duration = Duration::from_secs(1);
/// Quiet gap that ends a burst of file-change events.
const TOKEN_DEBOUNCE: Duration = Duration::from_millis(80);
const TOKEN_MAX_BURST: Duration = Duration::from_millis(300);
/// Safety-net full rescan, for new folders or missed events.
const TOKEN_FULL_RESCAN: Duration = Duration::from_secs(30);
/// Claude Code's own usage view polls about this often; limits move slowly.
const CLAUDE_LIMITS_INTERVAL: Duration = Duration::from_secs(300);
/// After a failure (e.g. HTTP 429, too many requests), wait longer before retrying.
const CLAUDE_LIMITS_BACKOFF: Duration = Duration::from_secs(900);
const TRAY_ID: &str = "mika-tray";
/// Passed by the login item: start in the menu bar without opening the window.
const HIDDEN_ARG: &str = "--hidden";

/// The latest snapshot, so a window that opens between samples has data at once.
#[derive(Default)]
struct Latest(Mutex<Option<Snapshot>>);

/// Hook-reported session states, shared by the listener and the sampler loop.
#[derive(Default)]
struct Statuses(Mutex<Tracker>);

#[tauri::command]
fn latest_snapshot(latest: State<'_, Latest>) -> Option<Snapshot> {
    latest.0.lock().ok().and_then(|s| s.clone())
}

#[derive(Default)]
struct LatestClaudeLimits(Mutex<Option<ClaudeLimits>>);

#[tauri::command]
fn claude_limits(latest: State<'_, LatestClaudeLimits>) -> Option<ClaudeLimits> {
    latest.0.lock().ok().and_then(|s| s.clone())
}

#[derive(Default)]
struct LatestTokens(Mutex<Option<TokenStats>>);

/// User settings and the file they're saved to.
struct AppSettings {
    path: PathBuf,
    value: Mutex<Settings>,
}

fn current_settings(app: &AppHandle) -> Settings {
    app.state::<AppSettings>().value.lock().map(|s| s.clone()).unwrap_or_default()
}

#[tauri::command]
fn get_settings(app: AppHandle) -> Settings {
    current_settings(&app)
}

#[tauri::command]
fn set_settings(app: AppHandle, settings: Settings) -> Result<Settings, String> {
    let settings = settings.sanitized();
    let state = app.state::<AppSettings>();
    settings::save(&state.path, &settings)?;
    if let Ok(mut value) = state.value.lock() {
        *value = settings.clone();
    }
    let _ = app.emit("settings", &settings);
    publish_statuses(&app);
    Ok(settings)
}

#[tauri::command]
fn preview_sound(name: String) {
    if settings::SOUNDS.contains(&name.as_str()) {
        let _ = std::process::Command::new("afplay").arg(format!("/System/Library/Sounds/{name}.aiff")).spawn();
    }
}

/// "installed", "outdated" (an older Mika version's hooks) or "missing".
#[tauri::command]
fn hooks_status(app: AppHandle, agent: Integration) -> Result<&'static str, String> {
    let home = app.path().home_dir().map_err(|e| e.to_string())?;
    let file = settings::read_hooks_file(agent, &home);
    Ok(if settings::hooks_installed(agent, &file) {
        "installed"
    } else if settings::any_mika_hooks(&file) {
        "outdated"
    } else {
        "missing"
    })
}

#[tauri::command]
fn install_hooks(app: AppHandle, agent: Integration) -> Result<(), String> {
    let home = app.path().home_dir().map_err(|e| e.to_string())?;
    settings::update_hooks_file(agent, &home, |v| settings::with_hooks(agent, v))
}

#[tauri::command]
fn remove_hooks(app: AppHandle, agent: Integration) -> Result<(), String> {
    let home = app.path().home_dir().map_err(|e| e.to_string())?;
    settings::update_hooks_file(agent, &home, settings::without_hooks)
}

/// Pops up the "needs you" window with a repeating ringtone. Falls back to a
/// single chime when the user turned the popup off in settings.
#[tauri::command]
fn show_alert(app: AppHandle, info: AlertInfo) -> Result<(), String> {
    let settings = current_settings(&app);
    if !settings.alert_popup {
        alert(app, info.title, info.body);
        return Ok(());
    }
    let ring = (settings.sound_enabled && !settings.is_quiet_now()).then(|| settings.sound_path());
    alert::show(&app, info, ring).map_err(|e| e.to_string())
}

/// Brings an agent's session to the front (from the header or a row): its
/// terminal tab when it can be found, otherwise the app hosting it.
#[tauri::command]
fn focus_app(path: String, pid: Option<u32>) {
    thread::spawn(move || {
        alert::focus_agent(Some(&path), pid);
    });
}

/// Dismissing a permission request hands it back to the agent's own prompt.
#[tauri::command]
fn dismiss_alert(app: AppHandle) {
    match app.state::<AlertState>().permission().and_then(|a| a.request_id) {
        Some(id) => answer_permission(app, id, Choice::Terminal),
        None => alert::dismiss(&app),
    }
}

/// Goes to the session; for a permission request, hands it back to the
/// agent's prompt there.
#[tauri::command]
fn open_alert_target(app: AppHandle) {
    let Some(asking) = app.state::<AlertState>().permission() else {
        alert::open_target(&app);
        return;
    };
    if let Some(id) = asking.request_id {
        answer_permission(app, id, Choice::Terminal);
    }
    thread::spawn(move || {
        alert::focus_agent(asking.host_app.as_deref(), asking.pid);
    });
}

/// The user's answer to a permission request, from the popup.
#[tauri::command]
fn answer_permission(app: AppHandle, id: u64, choice: Choice) {
    app.state::<Permissions>().answer(id, choice);
}

#[tauri::command]
fn current_alert(state: State<'_, AlertState>) -> Option<AlertInfo> {
    state.current()
}

/// Gets the user's attention when an agent needs them: a sound that needs no
/// permission and a notification where macOS allows it (notifications from
/// scripts can be silently blocked, so they are a bonus). The sound follows
/// the user's alert settings and quiet hours.
#[tauri::command]
fn alert(app: AppHandle, title: String, body: String) {
    let settings = current_settings(&app);
    if settings.sound_enabled
        && !settings.is_quiet_now()
        && let Err(err) = std::process::Command::new("afplay").arg(settings.sound_path()).spawn()
    {
        eprintln!("mika: could not play alert sound: {err}");
    }
    let quote = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
    let script = format!("display notification \"{}\" with title \"{}\"", quote(&body), quote(&title));
    let _ = std::process::Command::new("osascript").args(["-e", &script]).spawn();
}

/// Lets the WebView report to the terminal, where webview console output doesn't reach.
#[tauri::command]
fn log_frontend(message: String) {
    eprintln!("mika ui: {message}");
}

#[tauri::command]
fn latest_tokens(latest: State<'_, LatestTokens>) -> Option<TokenStats> {
    latest.0.lock().ok().and_then(|s| s.clone())
}

#[tauri::command]
fn session_statuses(app: AppHandle) -> Vec<SessionStatus> {
    visible_statuses(&app).unwrap_or_default()
}

fn state_glyph(state: AgentState) -> &'static str {
    match state {
        AgentState::Idle => "○",
        AgentState::Done => "✓",
        AgentState::Thinking => "◌",
        AgentState::Working => "●",
        AgentState::Turn => "◇",
        AgentState::Waiting => "◆",
    }
}

/// `style` is the user's choice: "full", "compact" (symbol and count) or "icon" (no text).
fn tray_title(snapshot: &Snapshot, state: Option<AgentState>, style: &str) -> String {
    if style == "icon" {
        return String::new();
    }
    if snapshot.agents.is_empty() {
        return "Mika".into();
    }
    let glyph = state.map(|s| format!("{} ", state_glyph(s))).unwrap_or_default();
    if style == "compact" {
        return format!("{glyph}{}", snapshot.agents.len());
    }
    let cpu: f32 = snapshot.agents.iter().map(|a| a.cpu).sum();
    let mem: u64 = snapshot.agents.iter().map(|a| a.memory).sum();
    format!(
        "{glyph}{} agent{} · {:.0}% · {:.1} GB",
        snapshot.agents.len(),
        if snapshot.agents.len() == 1 { "" } else { "s" },
        cpu,
        mem as f64 / 1_073_741_824.0
    )
}

fn toggle_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        // Hide only when Mika is actually in front. A window that's open but
        // behind other apps, minimized or on another Space still reports
        // visible, and hiding it then made the user click twice.
        let in_front = window.is_visible().unwrap_or(false)
            && window.is_focused().unwrap_or(false)
            && !window.is_minimized().unwrap_or(false);
        if in_front {
            let _ = window.hide();
        } else {
            show_main_window(app);
        }
    }
}

fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn spawn_sampler(app: AppHandle) {
    thread::spawn(move || {
        let mut sampler = match Sampler::new() {
            Ok(sampler) => sampler,
            Err(err) => {
                eprintln!("mika: process sampler unavailable: {err}");
                return;
            }
        };
        loop {
            let snapshot = sampler.sample();
            let _ = app.emit("agents", &snapshot);
            if let Ok(mut latest) = app.state::<Latest>().0.lock() {
                *latest = Some(snapshot);
            }
            // Re-publishing statuses each tick lets done sessions settle to idle.
            publish_statuses(&app);
            thread::sleep(Duration::from_millis(current_settings(&app).sample_interval_ms));
        }
    });
}

/// Emits the current session statuses and refreshes the tray title.
/// Every session's status as the user sees it: with "your turn" turned off,
/// a session waiting for its next message just shows as idle.
fn visible_statuses(app: &AppHandle) -> Option<Vec<SessionStatus>> {
    let mut sessions = app.state::<Statuses>().0.lock().ok()?.snapshot(status::now_ms());
    if current_settings(app).turn_alert == "off" {
        for s in sessions.iter_mut().filter(|s| s.state == AgentState::Turn) {
            s.state = AgentState::Idle;
        }
    }
    Some(sessions)
}

fn publish_statuses(app: &AppHandle) {
    let Some(sessions) = visible_statuses(app) else { return };
    let _ = app.emit("statuses", &sessions);
    let title = match app.state::<Latest>().0.lock() {
        Ok(latest) => latest.as_ref().map(|s| tray_title(s, status::overall(&sessions), &current_settings(app).tray_title)),
        Err(_) => None,
    };
    if let (Some(title), Some(tray)) = (title, app.tray_by_id(TRAY_ID)) {
        let _ = tray.set_title(Some(title));
    }
}

fn spawn_token_scanner(app: AppHandle) {
    let Ok(home) = app.path().home_dir() else {
        eprintln!("mika: no home directory, token usage unavailable");
        return;
    };
    thread::spawn(move || {
        let mut scanner = TokenScanner::new(&home);
        let publish = |scanner: &mut TokenScanner| {
            let stats = scanner.stats();
            let _ = app.emit("tokens", &stats);
            if let Ok(mut latest) = app.state::<LatestTokens>().0.lock() {
                *latest = Some(stats);
            }
        };
        scanner.scan();
        publish(&mut scanner);

        // FSEvents tells us which log changed, so updates land within ~100 ms.
        let (tx, rx) = mpsc::channel::<Vec<PathBuf>>();
        let watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(event) = res {
                let _ = tx.send(event.paths);
            }
        });
        let _watcher = match watcher {
            Ok(mut w) => {
                for target in scanner.watch_targets() {
                    let _ = w.watch(&target, RecursiveMode::Recursive);
                }
                Some(w)
            }
            Err(err) => {
                eprintln!("mika: file watching unavailable, polling instead: {err}");
                None
            }
        };

        let mut last_full = Instant::now();
        loop {
            match rx.recv_timeout(TOKEN_TICK) {
                Ok(first) => {
                    // Agents write in bursts; gather the burst into one read.
                    // Capped, so a constantly writing agent can't hold updates back.
                    let mut paths: HashSet<PathBuf> = first.into_iter().collect();
                    let burst_end = Instant::now() + TOKEN_MAX_BURST;
                    while let Some(left) = burst_end.checked_duration_since(Instant::now()) {
                        match rx.recv_timeout(left.min(TOKEN_DEBOUNCE)) {
                            Ok(more) => paths.extend(more),
                            Err(_) => break,
                        }
                    }
                    scanner.scan_paths(paths);
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => thread::sleep(TOKEN_TICK),
            }
            // A full rescan catches new folders or events the watcher missed.
            if last_full.elapsed() >= TOKEN_FULL_RESCAN {
                scanner.scan();
                last_full = Instant::now();
            }
            // Published every tick too, so tokens per second decays to zero.
            publish(&mut scanner);
        }
    });
}

fn spawn_claude_limits(app: AppHandle) {
    thread::spawn(move || {
        let mut last_error = None;
        loop {
            // Off means no Keychain access and no network request at all.
            if !current_settings(&app).claude_limits {
                thread::sleep(Duration::from_secs(5));
                continue;
            }
            let wait = match claude_limits::fetch() {
                Ok(limits) => {
                    last_error = None;
                    let _ = app.emit("claude_limits", &limits);
                    if let Ok(mut latest) = app.state::<LatestClaudeLimits>().0.lock() {
                        *latest = Some(limits);
                    }
                    CLAUDE_LIMITS_INTERVAL
                }
                Err(err) => {
                    // Log each distinct failure once rather than every retry.
                    if last_error.as_ref() != Some(&err) {
                        eprintln!("mika: Claude limits unavailable: {err}");
                        last_error = Some(err);
                    }
                    CLAUDE_LIMITS_BACKOFF
                }
            };
            thread::sleep(wait);
        }
    });
}

/// Alerts when a session starts waiting for the user, or finishes if the user
/// asked for that. Runs here rather than in the window, because macOS pauses
/// a hidden window's page and its alerts would never fire.
fn alert_on_change(app: &AppHandle, before: Option<AgentState>, now: &SessionStatus) {
    if before == Some(now.state) {
        return;
    }
    let settings = current_settings(app);
    let tracked = match now.agent {
        "codex" => settings.track_codex,
        _ => settings.track_claude,
    };
    if !tracked {
        return;
    }
    let label = if now.agent == "codex" { "Codex" } else { "Claude Code" };
    let folder = short_path(now.cwd.as_deref());
    let (host_app, pid) = host_for(app, now.agent, now.cwd.as_deref()).unzip();
    let info = |kind, title: String, body: String| AlertInfo {
        kind,
        session_id: now.session_id.clone(),
        agent: now.agent.to_string(),
        title,
        body,
        tool: now.tool.clone(),
        host_app: host_app.clone().flatten(),
        pid,
        since_ms: now.since_ms,
        request_id: None,
        detail: None,
        can_always: false,
    };
    let busy = app.state::<AlertState>().waiting_on_other(&now.session_id);
    let result = match now.state {
        // Nothing is blocked: at most one chime, never the popup.
        AgentState::Turn => {
            if settings.turn_alert == "chime" && settings.sound_enabled && !settings.is_quiet_now() {
                let _ = std::process::Command::new("afplay").arg(settings.sound_path()).spawn();
            }
            Ok(())
        }
        // Never cover a popup still waiting on an answer for another session.
        AgentState::Waiting if busy => {
            alert(app.clone(), format!("{label} needs you"), folder);
            Ok(())
        }
        AgentState::Waiting => show_alert(app.clone(), info(AlertKind::Waiting, format!("{label} needs you"), folder)),
        AgentState::Done if settings.alert_on_done && !busy => {
            show_alert(app.clone(), info(AlertKind::Done, format!("{label} finished"), folder))
        }
        AgentState::Done if settings.alert_on_done => {
            alert(app.clone(), "Mika · an agent finished".into(), format!("{label} in {folder} is done."));
            Ok(())
        }
        _ => Ok(()),
    };
    if let Err(err) = result {
        eprintln!("mika: alert popup failed: {err}");
    }
}

/// The agent process running a session and the app hosting it, matched like
/// the window does.
fn host_for(app: &AppHandle, agent: &str, cwd: Option<&str>) -> Option<(Option<String>, u32)> {
    let latest = app.state::<Latest>();
    let snapshot = latest.0.lock().ok()?;
    snapshot.as_ref()?.agents.iter().find_map(|a| {
        let same = match a.kind {
            AgentKind::Codex => agent == "codex",
            AgentKind::Claude => agent == "claude" && a.cwd.as_deref() == cwd,
        };
        same.then(|| (a.host_app.clone(), a.pid))
    })
}

fn short_path(path: Option<&str>) -> String {
    let Some(path) = path else { return "unknown folder".into() };
    let home = std::env::var("HOME").unwrap_or_default();
    if path == "/" || path.trim_end_matches('/') == home {
        return "no project folder".into();
    }
    match path.strip_prefix(&home) {
        Some(rest) if !home.is_empty() => format!("~{rest}"),
        _ => path.to_string(),
    }
}

/// Applies a hook event to the session tracker, returning the session's state
/// before and its status after.
fn track(app: &AppHandle, event: HookEvent) -> Option<(Option<AgentState>, Option<SessionStatus>)> {
    let id = event.session_id.clone();
    let change = app.state::<Statuses>().0.lock().ok().map(|mut tracker| {
        let before = tracker.get(&id).map(|s| s.state);
        tracker.apply(event, status::now_ms());
        (before, tracker.get(&id).cloned())
    });
    publish_statuses(app);
    change
}

fn spawn_hook_listener(app: AppHandle) {
    thread::spawn(move || {
        let result = status::serve(
            |event| {
                if let Some((before, Some(now))) = track(&app, event) {
                    alert_on_change(&app, before, &now);
                }
            },
            |ask, request| handle_permission(&app, ask, request),
        );
        if let Err(err) = result {
            eprintln!("mika: hook listener on {} unavailable: {err}", status::HOOK_ADDR);
        }
    });
}

/// A permission request from an agent's hook. The session shows as waiting
/// either way; when the user lets Mika answer, the popup asks and the hook's
/// HTTP request stays open until they choose (on its own thread, so other
/// hooks keep flowing). Otherwise it's answered with nothing at once, and the
/// agent asks in the terminal as usual.
fn handle_permission(app: &AppHandle, ask: permission::Request, request: tiny_http::Request) {
    let waiting = HookEvent {
        agent: ask.agent,
        session_id: ask.session_id.clone(),
        hook_event_name: "PermissionRequest".into(),
        cwd: ask.cwd.clone(),
        tool_name: Some(ask.tool_name.clone()),
        notification_type: None,
    };
    let change = track(app, waiting);
    let settings = current_settings(app);
    let tracked = if ask.agent == "codex" { settings.track_codex } else { settings.track_claude };
    if !(tracked && settings.alert_popup && settings.answer_from_mika) {
        let _ = request.respond(tiny_http::Response::empty(204));
        if let Some((before, Some(now))) = change {
            alert_on_change(app, before, &now);
        }
        return;
    }

    let (reply, choice) = mpsc::channel();
    let id = app.state::<Permissions>().add(ask.clone(), reply);
    // One request at a time; the others wait their turn in the queue.
    if app.state::<AlertState>().permission().is_none() {
        show_permission(app, id, &ask);
    }
    let app = app.clone();
    thread::spawn(move || {
        let choice = choice.recv_timeout(permission::WAIT).unwrap_or(Choice::Terminal);
        app.state::<Permissions>().remove(id);
        let response = match permission::decision_json(choice, &ask) {
            Some(json) => tiny_http::Response::from_string(json)
                .with_header(tiny_http::Header::from_bytes("Content-Type", "application/json").expect("valid header"))
                .boxed(),
            None => tiny_http::Response::empty(204).boxed(),
        };
        if let Err(err) = request.respond(response) {
            eprintln!("mika: could not answer the permission hook: {err}");
        }
        // Answered here: the agent carries on, so it's no longer waiting.
        if choice != Choice::Terminal {
            track(&app, HookEvent {
                agent: ask.agent,
                session_id: ask.session_id.clone(),
                hook_event_name: "PostToolUse".into(),
                cwd: None,
                tool_name: None,
                notification_type: None,
            });
        }
        let showing = app.state::<AlertState>().permission().and_then(|a| a.request_id);
        if showing == Some(id) {
            match app.state::<Permissions>().oldest() {
                Some((next, next_ask)) => show_permission(&app, next, &next_ask),
                None => alert::dismiss(&app),
            }
        }
    });
}

fn show_permission(app: &AppHandle, id: u64, ask: &permission::Request) {
    let settings = current_settings(app);
    let label = if ask.agent == "codex" { "Codex" } else { "Claude Code" };
    let (host_app, pid) = host_for(app, ask.agent, ask.cwd.as_deref()).unzip();
    let info = AlertInfo {
        kind: AlertKind::Permission,
        session_id: ask.session_id.clone(),
        agent: ask.agent.to_string(),
        title: format!("{label} wants to use {}", ask.tool_name),
        body: short_path(ask.cwd.as_deref()),
        tool: Some(ask.tool_name.clone()),
        host_app: host_app.flatten(),
        pid,
        since_ms: status::now_ms(),
        request_id: Some(id),
        detail: Some(permission::detail(ask)).filter(|d| !d.is_empty()),
        can_always: permission::can_always(ask),
    };
    let ring = (settings.sound_enabled && !settings.is_quiet_now()).then(|| settings.sound_path());
    if let Err(err) = alert::show(app, info, ring) {
        eprintln!("mika: permission popup failed: {err}");
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, Some(vec![HIDDEN_ARG])))
        .manage(Latest::default())
        .manage(Statuses::default())
        .manage(LatestTokens::default())
        .manage(LatestClaudeLimits::default())
        .manage(AlertState::default())
        .manage(Permissions::default())
        .invoke_handler(tauri::generate_handler![latest_snapshot, session_statuses, latest_tokens, claude_limits, alert, log_frontend, get_settings, set_settings, preview_sound, hooks_status, install_hooks, remove_hooks, show_alert, dismiss_alert, open_alert_target, current_alert, focus_app, answer_permission])
        .setup(|app| {
            // Menu bar only: no Dock icon and no Cmd+Tab entry. Bundled builds
            // also set LSUIElement (Info.plist) so the Dock icon never flashes.
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let show = MenuItem::with_id(app, "show", "Show Mika", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit Mika", true, None::<&str>)?;
            // Mika has no visible app menu (menu bar only), so About lives here.
            let config = app.config();
            let about = PredefinedMenuItem::about(
                app,
                Some("About Mika"),
                Some(AboutMetadata {
                    name: Some("Mika".into()),
                    version: Some(app.package_info().version.to_string()),
                    authors: config.bundle.publisher.clone().map(|p| vec![p]),
                    copyright: config.bundle.copyright.clone(),
                    ..Default::default()
                }),
            )?;
            let separator = PredefinedMenuItem::separator(app)?;
            let menu = Menu::with_items(app, &[&show, &about, &separator, &quit])?;

            TrayIconBuilder::with_id(TRAY_ID)
                // A template silhouette: macOS tints it to match the menu bar.
                .icon(tauri::image::Image::from_bytes(include_bytes!("../icons/tray-icon.png"))?)
                .icon_as_template(true)
                .title("Mika")
                .tooltip("Mika — agent monitor")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => toggle_main_window(app),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        toggle_main_window(tray.app_handle());
                    }
                })
                .build(app)?;

            // The default menu's Quit (Cmd+Q) terminates the process outright:
            // tao never asks first, so ExitRequested can't stop it. Swap it for
            // an item that hides Mika to the menu bar; the tray's "Quit Mika"
            // still quits.
            #[cfg(target_os = "macos")]
            {
                let menu = Menu::default(app.handle())?;
                if let Some(app_menu) = menu.items()?.first().and_then(|item| item.as_submenu().cloned()) {
                    // Quit is the last item of the app menu.
                    let quit_at = app_menu.items()?.len().saturating_sub(1);
                    app_menu.remove_at(quit_at)?;
                    app_menu.append(&MenuItem::with_id(app, "hide-to-menu-bar", "Hide to Menu Bar", true, Some("CmdOrCtrl+Q"))?)?;
                }
                app.set_menu(menu)?;
            }

            // The login item stores the path of whichever copy of Mika turned
            // it on: maybe a dev build or an app that has since moved. Release
            // builds point it back at themselves so login always opens this app.
            #[cfg(not(debug_assertions))]
            {
                use tauri_plugin_autostart::ManagerExt;
                let autolaunch = app.autolaunch();
                if autolaunch.is_enabled().unwrap_or(false)
                    && let Err(err) = autolaunch.enable()
                {
                    eprintln!("mika: could not refresh the login item: {err}");
                }
            }

            let settings_path = app.path().app_config_dir()?.join("settings.json");
            app.manage(AppSettings { value: Mutex::new(settings::load(&settings_path)), path: settings_path });
            // The window starts invisible (tauri.conf.json) so a login launch
            // never flashes it; any other launch shows it now.
            if !std::env::args().any(|arg| arg == HIDDEN_ARG) {
                show_main_window(app.handle());
            }

            spawn_sampler(app.handle().clone());
            spawn_hook_listener(app.handle().clone());
            spawn_token_scanner(app.handle().clone());
            spawn_claude_limits(app.handle().clone());
            Ok(())
        })
        .on_menu_event(|app, event| {
            if event.id.as_ref() == "hide-to-menu-bar"
                && let Some(window) = app.get_webview_window("main")
            {
                let _ = window.hide();
            }
        })
        .on_window_event(|window, event| {
            // Closing or minimizing the main window hides it; Mika keeps
            // running in the menu bar. Tauri has no minimize event, but
            // minimizing fires a resize.
            match event {
                tauri::WindowEvent::CloseRequested { api, .. } => {
                    api.prevent_close();
                    let _ = window.hide();
                }
                tauri::WindowEvent::Resized(_) if window.label() == "main" && window.is_minimized().unwrap_or(false) => {
                    let _ = window.hide();
                }
                _ => {}
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app, _event| {
            // Opening Mika again (Finder, Spotlight) while it runs shows the window.
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { .. } = _event {
                show_main_window(_app);
            }
        });
}
