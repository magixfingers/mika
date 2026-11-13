//! The alert popup: a small always-on-top window ("needs you" with a repeating
//! ringtone, or "finished" with a single chime), and a way to jump straight to
//! the terminal tab or app running the session that raised it.

use std::process::Command;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, WebviewUrl, WebviewWindowBuilder};

const WINDOW: &str = "alert";
const WIDTH: f64 = 380.0;
const HEIGHT: f64 = 156.0;
/// A permission request also shows what the agent wants to run.
const PERMISSION_HEIGHT: f64 = 248.0;
const MARGIN: f64 = 16.0;
/// The ringtone plays this many times, this far apart; the popup itself stays
/// until the user responds.
const RING_GAP: Duration = Duration::from_secs(2);
const MAX_RINGS: u32 = 3;
/// A "finished" popup needs no answer, so it clears itself after this long.
const DONE_LINGER: Duration = Duration::from_secs(12);

/// Why the popup is up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AlertKind {
    Waiting,
    Done,
    /// A permission request the user can answer right in the popup.
    Permission,
}

/// What the popup shows, raised when a session starts waiting or finishes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertInfo {
    pub kind: AlertKind,
    pub session_id: String,
    pub agent: String,
    pub title: String,
    pub body: String,
    pub tool: Option<String>,
    pub host_app: Option<String>,
    /// The agent process, used to find its exact terminal tab.
    pub pid: Option<u32>,
    pub since_ms: u64,
    /// For a permission request: its id, what it wants to run, and whether
    /// "Always allow" has a rule to save.
    pub request_id: Option<u64>,
    pub detail: Option<String>,
    pub can_always: bool,
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

    /// The permission request the popup is showing, if any.
    pub fn permission(&self) -> Option<AlertInfo> {
        self.current().filter(|a| a.kind == AlertKind::Permission)
    }

    /// True while a popup that needs an answer is up for another session.
    pub fn waiting_on_other(&self, session_id: &str) -> bool {
        self.current()
            .is_some_and(|a| a.kind != AlertKind::Done && a.session_id != session_id)
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
    let height = if info.kind == AlertKind::Permission { PERMISSION_HEIGHT } else { HEIGHT };
    window.set_size(tauri::LogicalSize::new(WIDTH, height))?;
    // Top-right corner of the screen the mouse is on (that's where the user is
    // looking), under the menu bar. Monitor positions are in global physical
    // pixels, so place the window in the same space.
    let monitor = app
        .cursor_position()
        .ok()
        .and_then(|p| app.monitor_from_point(p.x, p.y).ok().flatten())
        .or(app.primary_monitor()?);
    if let Some(monitor) = monitor {
        let scale = monitor.scale_factor();
        let (origin, size) = (monitor.position(), monitor.size());
        let x = origin.x as f64 + size.width as f64 - (WIDTH + MARGIN) * scale;
        let y = origin.y as f64 + 40.0 * scale;
        window.set_position(PhysicalPosition::new(x.round() as i32, y.round() as i32))?;
    }
    window.show()?;
    window.set_focus()?;
    let _ = app.emit_to(WINDOW, "alert", &info);

    if info.kind == AlertKind::Done {
        let app = app.clone();
        thread::spawn(move || {
            thread::sleep(DONE_LINGER);
            if app.state::<AlertState>().generation.load(Ordering::SeqCst) == generation {
                dismiss(&app);
            }
        });
    }

    if let Some(sound) = ring {
        // A finished session gets one chime; a waiting one rings until answered.
        let rings = if info.kind == AlertKind::Done { 1 } else { MAX_RINGS };
        let app = app.clone();
        thread::spawn(move || {
            for _ in 0..rings {
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

/// Brings an app to the front. Only ever opens an `.app` bundle that exists,
/// never an arbitrary path; returns false when there's nothing to open.
pub fn focus_app(path: &str) -> bool {
    if !is_app_bundle(path) {
        return false;
    }
    match Command::new("open").arg("-a").arg(path).spawn() {
        Ok(_) => true,
        Err(err) => {
            eprintln!("mika: could not open {path}: {err}");
            false
        }
    }
}

fn is_app_bundle(path: &str) -> bool {
    path.ends_with(".app") && std::path::Path::new(path).is_dir()
}

/// Brings the agent's own session to the front: the exact Terminal or iTerm2
/// tab it runs in when that can be found, otherwise the app hosting it.
/// Blocks on AppleScript, so call it off the main thread.
pub fn focus_agent(host_app: Option<&str>, pid: Option<u32>) -> bool {
    let Some(path) = host_app.filter(|p| is_app_bundle(p)) else { return false };
    if let Some(id) = window_app_for(path) {
        return focus_bundle_id(id) || focus_app(path);
    }
    if let Some(tty) = pid.and_then(tty_of)
        && let Some(script) = select_tab_script(path, &tty)
    {
        match Command::new("osascript").args(["-e", &script]).output() {
            Ok(out) if String::from_utf8_lossy(&out.stdout).trim() == "true" => return true,
            Ok(out) => eprintln!(
                "mika: no tab on {tty} in {path}: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ),
            Err(err) => eprintln!("mika: could not run osascript: {err}"),
        }
    }
    focus_app(path)
}

/// Some hosts run agents in a background helper app whose window lives in a
/// separate app: activating the helper would show nothing. Maps the helper's
/// bundle name to the bundle id of the app with the window.
fn window_app_for(app_path: &str) -> Option<&'static str> {
    match app_path.rsplit('/').next()? {
        // Herdr keeps its terminals alive in Herdr Core; Herdr.app shows them.
        "Herdr Core.app" => Some("dev.herdr.app"),
        _ => None,
    }
}

fn focus_bundle_id(id: &str) -> bool {
    Command::new("open").args(["-b", id]).status().is_ok_and(|s| s.success())
}

/// The controlling terminal of a process, e.g. `/dev/ttys003`.
fn tty_of(pid: u32) -> Option<String> {
    let out = Command::new("ps").args(["-o", "tty=", "-p", &pid.to_string()]).output().ok()?;
    tty_path(&String::from_utf8_lossy(&out.stdout))
}

fn tty_path(ps_tty: &str) -> Option<String> {
    let name = ps_tty.trim();
    // `??` means no terminal; anything else odd is never put into a script.
    let valid = name.starts_with("tty") && name.chars().all(|c| c.is_ascii_alphanumeric());
    valid.then(|| format!("/dev/{name}"))
}

/// AppleScript that selects the tab whose terminal is `tty` and brings its
/// window to the front, printing whether it found one. None for apps that
/// can't be scripted that way.
fn select_tab_script(app_path: &str, tty: &str) -> Option<String> {
    let bundle = app_path.rsplit('/').next()?;
    let body = match bundle {
        "Terminal.app" => format!(
            r#"tell application id "com.apple.Terminal"
  repeat with w in windows
    repeat with t in tabs of w
      if tty of t is "{tty}" then
        set selected tab of w to t
        set index of w to 1
        activate
        return true
      end if
    end repeat
  end repeat
end tell
return false"#
        ),
        "iTerm.app" | "iTerm2.app" => format!(
            r#"tell application id "com.googlecode.iterm2"
  repeat with w in windows
    repeat with t in tabs of w
      repeat with s in sessions of t
        if tty of s is "{tty}" then
          select w
          select t
          select s
          activate
          return true
        end if
      end repeat
    end repeat
  end repeat
end tell
return false"#
        ),
        _ => return None,
    };
    Some(body)
}

/// Brings the session that raised the popup to the front, then closes it.
pub fn open_target(app: &AppHandle) {
    let current = app.state::<AlertState>().current();
    dismiss(app);
    let app = app.clone();
    thread::spawn(move || {
        let focused = current.is_some_and(|a| focus_agent(a.host_app.as_deref(), a.pid));
        if !focused {
            // No host app known: show Mika itself instead.
            let main_app = app.clone();
            let _ = app.run_on_main_thread(move || {
                if let Some(main) = main_app.get_webview_window("main") {
                    let _ = main.show();
                    let _ = main.set_focus();
                }
            });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_tty_from_ps() {
        assert_eq!(tty_path("ttys003\n").as_deref(), Some("/dev/ttys003"));
        assert_eq!(tty_path("??"), None);
        assert_eq!(tty_path("ttys0\"; rm"), None);
    }

    #[test]
    fn scripts_only_known_terminals() {
        assert!(select_tab_script("/System/Applications/Utilities/Terminal.app", "/dev/ttys001").is_some());
        assert!(select_tab_script("/Applications/iTerm.app", "/dev/ttys001").is_some());
        assert!(select_tab_script("/Applications/Visual Studio Code.app", "/dev/ttys001").is_none());
    }

    #[test]
    fn maps_background_hosts_to_their_window_app() {
        let core = "/Users/me/.local/share/herdr-core/active/Herdr Core.app";
        assert_eq!(window_app_for(core), Some("dev.herdr.app"));
        assert_eq!(window_app_for("/System/Applications/Utilities/Terminal.app"), None);
    }
}
