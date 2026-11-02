mod claude_limits;
mod sampler;
mod status;
mod tokens;

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use notify::{RecursiveMode, Watcher};

use claude_limits::ClaudeLimits;
use sampler::{Sampler, Snapshot};
use status::{AgentState, SessionStatus, Tracker};
use tokens::{TokenScanner, TokenStats};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, State};

const SAMPLE_INTERVAL: Duration = Duration::from_secs(1);
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

const ALERT_SOUND: &str = "/System/Library/Sounds/Glass.aiff";

/// Gets the user's attention when an agent needs them: a sound that needs no
/// permission, a bouncing Dock icon, and a notification where macOS allows it
/// (notifications from scripts can be silently blocked, so they are a bonus).
#[tauri::command]
fn alert(app: AppHandle, title: String, body: String) {
    if let Err(err) = std::process::Command::new("afplay").arg(ALERT_SOUND).spawn() {
        eprintln!("mika: could not play alert sound: {err}");
    }
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.request_user_attention(Some(tauri::UserAttentionType::Critical));
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
fn session_statuses(statuses: State<'_, Statuses>) -> Vec<SessionStatus> {
    statuses.0.lock().map(|mut t| t.snapshot(status::now_ms())).unwrap_or_default()
}

fn state_glyph(state: AgentState) -> &'static str {
    match state {
        AgentState::Idle => "○",
        AgentState::Done => "✓",
        AgentState::Thinking => "◌",
        AgentState::Working => "●",
        AgentState::Waiting => "◆",
    }
}

fn tray_title(snapshot: &Snapshot, state: Option<AgentState>) -> String {
    if snapshot.agents.is_empty() {
        return "Mika".into();
    }
    let glyph = state.map(|s| format!("{} ", state_glyph(s))).unwrap_or_default();
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
        if window.is_visible().unwrap_or(false) {
            let _ = window.hide();
        } else {
            let _ = window.show();
            let _ = window.set_focus();
        }
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
            thread::sleep(SAMPLE_INTERVAL);
        }
    });
}

/// Emits the current session statuses and refreshes the tray title.
fn publish_statuses(app: &AppHandle) {
    let sessions = match app.state::<Statuses>().0.lock() {
        Ok(mut tracker) => tracker.snapshot(status::now_ms()),
        Err(_) => return,
    };
    let _ = app.emit("statuses", &sessions);
    let title = match app.state::<Latest>().0.lock() {
        Ok(latest) => latest.as_ref().map(|s| tray_title(s, status::overall(&sessions))),
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

fn spawn_hook_listener(app: AppHandle) {
    thread::spawn(move || {
        let result = status::serve(|event| {
            if let Ok(mut tracker) = app.state::<Statuses>().0.lock() {
                tracker.apply(event, status::now_ms());
            }
            publish_statuses(&app);
        });
        if let Err(err) = result {
            eprintln!("mika: hook listener on {} unavailable: {err}", status::HOOK_ADDR);
        }
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(Latest::default())
        .manage(Statuses::default())
        .manage(LatestTokens::default())
        .manage(LatestClaudeLimits::default())
        .invoke_handler(tauri::generate_handler![latest_snapshot, session_statuses, latest_tokens, claude_limits, alert, log_frontend])
        .setup(|app| {
            let show = MenuItem::with_id(app, "show", "Show Mika", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit Mika", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &quit])?;

            TrayIconBuilder::with_id(TRAY_ID)
                .icon(app.default_window_icon().cloned().expect("app icon"))
                .icon_as_template(false)
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

            spawn_sampler(app.handle().clone());
            spawn_hook_listener(app.handle().clone());
            spawn_token_scanner(app.handle().clone());
            spawn_claude_limits(app.handle().clone());
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the window hides it; Mika keeps running in the menu bar.
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
