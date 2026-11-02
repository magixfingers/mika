mod sampler;
mod status;
mod tokens;

use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use sampler::{Sampler, Snapshot};
use status::{AgentState, SessionStatus, Tracker};
use tokens::{TokenScanner, TokenStats};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, State};

const SAMPLE_INTERVAL: Duration = Duration::from_secs(2);
/// Token logs change less often than processes, and a scan touches many files.
const TOKEN_SCAN_INTERVAL: Duration = Duration::from_secs(5);
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
struct LatestTokens(Mutex<Option<TokenStats>>);

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
        loop {
            scanner.scan();
            // Emitted every tick, not only on change, so tokens per second decays to zero.
            let stats = scanner.stats();
            let _ = app.emit("tokens", &stats);
            if let Ok(mut latest) = app.state::<LatestTokens>().0.lock() {
                *latest = Some(stats);
            }
            thread::sleep(TOKEN_SCAN_INTERVAL);
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
        .invoke_handler(tauri::generate_handler![latest_snapshot, session_statuses, latest_tokens])
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
