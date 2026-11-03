//! The "needs you" popup: a small always-on-top window with a repeating
//! ringtone, and a way to jump to the app running the waiting session.

use std::process::Command;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, LogicalPosition, Manager, WebviewUrl, WebviewWindowBuilder};

const WINDOW: &str = "alert";
const WIDTH: f64 = 380.0;
const HEIGHT: f64 = 156.0;
const MARGIN: f64 = 16.0;
/// The ringtone repeats this often, and gives up after this many rings (about 30 s).
const RING_GAP: Duration = Duration::from_secs(2);
const MAX_RINGS: u32 = 15;

/// What the popup shows, sent by the UI when a session starts waiting.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertInfo {
    pub session_id: String,
    pub agent: String,
    pub title: String,
    pub body: String,
    pub tool: Option<String>,
    pub host_app: Option<String>,
    pub since_ms: u64,
}

#[derive(Default)]
pub struct AlertState {
    current: Mutex<Option<AlertInfo>>,
    /// Bumped on every show or dismiss, so an older ringtone loop knows to stop.
    generation: AtomicU64,
}

impl AlertState {
    pub fn current(&self) -> Option<AlertInfo> {
        self.current.lock().ok().and_then(|c| c.clone())
    }
}

/// Shows (or updates) the popup and starts the ringtone.
pub fn show(app: &AppHandle, info: AlertInfo, ring: Option<String>) -> tauri::Result<()> {
    let state = app.state::<AlertState>();
    if let Ok(mut current) = state.current.lock() {
        *current = Some(info.clone());
    }
    let generation = state.generation.fetch_add(1, Ordering::SeqCst) + 1;

    let window = match app.get_webview_window(WINDOW) {
        Some(window) => window,
        None => WebviewWindowBuilder::new(app, WINDOW, WebviewUrl::App("index.html#alert".into()))
            .title("Mika")
            .inner_size(WIDTH, HEIGHT)
            .resizable(false)
            .decorations(false)
            .always_on_top(true)
            .visible_on_all_workspaces(true)
            .skip_taskbar(true)
            .shadow(true)
            .visible(false)
            .build()?,
    };
    // Top-right corner of the main screen, under the menu bar.
    if let Some(monitor) = app.primary_monitor()? {
        let scale = monitor.scale_factor();
        let width = monitor.size().width as f64 / scale;
        window.set_position(LogicalPosition::new(width - WIDTH - MARGIN, 40.0))?;
    }
    window.show()?;
    window.set_focus()?;
    let _ = app.emit_to(WINDOW, "alert", &info);

    if let Some(sound) = ring {
        let app = app.clone();
        thread::spawn(move || {
            for _ in 0..MAX_RINGS {
                if app.state::<AlertState>().generation.load(Ordering::SeqCst) != generation {
                    break;
                }
                let _ = Command::new("afplay").arg(&sound).status();
                thread::sleep(RING_GAP);
            }
        });
    }
    Ok(())
}

/// Closes the popup and stops the ringtone.
pub fn dismiss(app: &AppHandle) {
    let state = app.state::<AlertState>();
    if let Ok(mut current) = state.current.lock() {
        *current = None;
    }
    state.generation.fetch_add(1, Ordering::SeqCst);
    if let Some(window) = app.get_webview_window(WINDOW) {
        let _ = window.hide();
    }
}

/// Brings the app running the session to the front, then closes the popup.
pub fn open_target(app: &AppHandle) {
    let target = app.state::<AlertState>().current().and_then(|a| a.host_app);
    match target {
        // Only ever open an .app bundle that exists, never an arbitrary path.
        Some(path) if path.ends_with(".app") && std::path::Path::new(&path).is_dir() => {
            if let Err(err) = Command::new("open").arg("-a").arg(&path).spawn() {
                eprintln!("mika: could not open {path}: {err}");
            }
        }
        _ => {
            // No host app known: show Mika itself instead.
            if let Some(main) = app.get_webview_window("main") {
                let _ = main.show();
                let _ = main.set_focus();
            }
        }
    }
    dismiss(app);
}
