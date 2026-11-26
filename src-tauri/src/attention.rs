//! Agents waiting on the user, and the menu-bar ghost that calls for them.
//!
//! When a session finishes, hands the turn back, or blocks on the user, it
//! joins the attention list. While the list has anything in it, the tray
//! ghost takes the most urgent item's color and bobs until the user clicks
//! it; the click opens a small popup under the icon listing who needs them.
//! An item leaves the list when the user opens it, clears the list, or the
//! agent gets back to work on its own.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use serde::Serialize;
use tauri::image::Image;
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, Rect, WebviewUrl, WebviewWindowBuilder};

use crate::status::AgentState;

const WINDOW: &str = "attention";
const WIDTH: f64 = 300.0;
/// Eyebrow and padding, each row, and the footer, in points.
const CHROME_HEIGHT: f64 = 86.0;
const ROW_HEIGHT: f64 = 50.0;
const MAX_ROWS: usize = 6;

/// Tray icon frames: 44 px (22 pt at 2x), one bob cycle.
const ICON_PX: u32 = 44;
const FRAMES: usize = 12;
const FRAME_TIME: Duration = Duration::from_millis(70);
const IDLE_POLL: Duration = Duration::from_millis(250);
/// How far the ghost bobs, in SVG units of the 62-unit canvas.
const BOB: f32 = 3.0;

/// The ghost from tray-icon.svg, eyes cut out.
const GHOST_PATH: &str = "M32 6c14 0 24 11 24 26v20c0 4-3 6-6 4l-4-3-5 4-5-4-4 4-4-4-5 4-5-4-4 3c-3 2-6 0-6-4V32C8 17 18 6 32 6z M21 30a3 4 0 1 0 6 0a3 4 0 1 0 -6 0z M37 30a3 4 0 1 0 6 0a3 4 0 1 0 -6 0z";

#[derive(Debug, Clone, Serialize)]
pub struct Item {
    pub session_id: String,
    /// "claude" or "codex".
    pub agent: &'static str,
    pub cwd: Option<String>,
    /// The folder as the user knows it, e.g. `~/DEVELOPMENT/mika`.
    pub folder: String,
    /// Done, Turn or Waiting.
    pub state: AgentState,
    pub since_ms: u64,
}

#[derive(Default)]
pub struct Attention {
    items: Mutex<Vec<Item>>,
    /// Set once the user has clicked the ghost; it stops bobbing but keeps
    /// its color until the list is empty. A new arrival starts it again.
    seen: AtomicBool,
}

impl Attention {
    pub fn items(&self) -> Vec<Item> {
        self.items.lock().map(|i| i.clone()).unwrap_or_default()
    }

    pub fn is_empty(&self) -> bool {
        self.items.lock().map(|i| i.is_empty()).unwrap_or(true)
    }

    /// Adds or updates a session. Returns true when it is new to the list,
    /// so a session that finishes and then hands the turn back rings once.
    pub fn raise(&self, item: Item) -> bool {
        let Ok(mut items) = self.items.lock() else { return false };
        let fresh = match items.iter_mut().find(|i| i.session_id == item.session_id) {
            Some(existing) => {
                // Keep the first arrival time; the state may move Done → Turn.
                let escalated = item.state > existing.state;
                existing.state = item.state;
                existing.cwd = item.cwd;
                existing.folder = item.folder;
                escalated && item.state == AgentState::Waiting
            }
            None => {
                items.push(item);
                true
            }
        };
        if fresh {
            self.seen.store(false, Ordering::SeqCst);
        }
        fresh
    }

    pub fn remove(&self, session_id: &str) -> Option<Item> {
        let mut items = self.items.lock().ok()?;
        let at = items.iter().position(|i| i.session_id == session_id)?;
        Some(items.remove(at))
    }

    pub fn clear(&self) {
        if let Ok(mut items) = self.items.lock() {
            items.clear();
        }
    }

    pub fn mark_seen(&self) {
        self.seen.store(true, Ordering::SeqCst);
    }

    /// The most urgent state in the list, which colors the ghost.
    fn urgent(&self) -> Option<AgentState> {
        self.items.lock().ok()?.iter().map(|i| i.state).max()
    }
}

/// Tells the popup the list changed and closes it once nothing is left.
pub fn publish(app: &AppHandle) {
    let attention = app.state::<Attention>();
    let items = attention.items();
    let _ = app.emit_to(WINDOW, "attention", &items);
    if let Some(window) = app.get_webview_window(WINDOW) {
        if items.is_empty() {
            let _ = window.hide();
        } else {
            let _ = window.set_size(tauri::LogicalSize::new(WIDTH, popup_height(items.len())));
        }
    }
}

fn popup_height(rows: usize) -> f64 {
    CHROME_HEIGHT + ROW_HEIGHT * rows.clamp(1, MAX_ROWS) as f64
}

/// Opens the list under the tray icon, or closes it if it's already open.
pub fn toggle_popup(app: &AppHandle, icon: Rect) -> tauri::Result<()> {
    if let Some(window) = app.get_webview_window(WINDOW)
        && window.is_visible().unwrap_or(false)
    {
        window.hide()?;
        return Ok(());
    }
    let window = match app.get_webview_window(WINDOW) {
        Some(window) => window,
        None => {
            let window = WebviewWindowBuilder::new(app, WINDOW, WebviewUrl::App("index.html#attention".into()))
                .title("Mika")
                .inner_size(WIDTH, popup_height(1))
                .resizable(false)
                .decorations(false)
                .always_on_top(true)
                .visible_on_all_workspaces(true)
                .skip_taskbar(true)
                .shadow(true)
                .visible(false)
                .build()?;
            // Like a menu: clicking anywhere else closes it.
            let closer = window.clone();
            window.on_window_event(move |event| {
                if let tauri::WindowEvent::Focused(false) = event {
                    let _ = closer.hide();
                }
            });
            window
        }
    };
    let rows = app.state::<Attention>().items().len();
    window.set_size(tauri::LogicalSize::new(WIDTH, popup_height(rows)))?;

    // Centered under the icon, kept on the icon's screen. The tray rect is in
    // global physical pixels, like monitor positions.
    let (position, size) = (icon.position.to_physical::<f64>(1.0), icon.size.to_physical::<f64>(1.0));
    let monitor = app.monitor_from_point(position.x, position.y).ok().flatten();
    let scale = monitor.as_ref().map_or(2.0, |m| m.scale_factor());
    let width = WIDTH * scale;
    let mut x = position.x + size.width / 2.0 - width / 2.0;
    if let Some(m) = &monitor {
        let (left, right) = (m.position().x as f64, m.position().x as f64 + m.size().width as f64);
        x = x.clamp(left + 8.0 * scale, right - width - 8.0 * scale);
    }
    let y = position.y + size.height + 4.0 * scale;
    window.set_position(PhysicalPosition::new(x.round() as i32, y.round() as i32))?;
    window.show()?;
    window.set_focus()?;
    let _ = app.emit_to(WINDOW, "attention", &app.state::<Attention>().items());
    Ok(())
}

pub fn hide_popup(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(WINDOW) {
        let _ = window.hide();
    }
}

/// The ghost as RGBA pixels, `dy` SVG units down. `color` None draws the
/// black template silhouette macOS tints to the menu bar.
fn ghost(color: Option<&str>, dy: f32) -> Option<Image<'static>> {
    let svg = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{ICON_PX}" height="{ICON_PX}" viewBox="1 0 62 62"><path transform="translate(0 {dy})" fill="{}" fill-rule="evenodd" d="{GHOST_PATH}"/></svg>"#,
        color.unwrap_or("#000")
    );
    let tree = resvg::usvg::Tree::from_str(&svg, &resvg::usvg::Options::default()).ok()?;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(ICON_PX, ICON_PX)?;
    resvg::render(&tree, resvg::tiny_skia::Transform::default(), &mut pixmap.as_mut());
    // tiny-skia stores premultiplied alpha; the tray wants straight RGBA.
    let rgba = pixmap
        .pixels()
        .iter()
        .flat_map(|p| {
            let c = p.demultiply();
            [c.red(), c.green(), c.blue(), c.alpha()]
        })
        .collect();
    Some(Image::new_owned(rgba, ICON_PX, ICON_PX))
}

/// The state colors from DESIGN.md: Alarm Red, Turn Violet, Done Mint.
fn color_for(state: AgentState) -> &'static str {
    match state {
        AgentState::Waiting => "#ee0000",
        AgentState::Turn => "#7928ca",
        _ => "#50e3c2",
    }
}

/// The template silhouette used when nobody needs the user.
pub fn resting_icon() -> Option<Image<'static>> {
    ghost(None, 0.0)
}

/// Runs for the life of the app: redraws the tray ghost when the list
/// changes and bobs it while the user hasn't looked yet.
pub fn spawn_animator(app: AppHandle, tray_id: &'static str, motion: impl Fn(&AppHandle) -> bool + Send + 'static) {
    thread::spawn(move || {
        let states = [AgentState::Done, AgentState::Turn, AgentState::Waiting];
        // Pre-render every frame once: a full bob cycle per state color.
        let frames: Vec<(AgentState, Vec<Image<'static>>)> = states
            .iter()
            .map(|&s| {
                let cycle = (0..FRAMES)
                    .filter_map(|f| {
                        let phase = f as f32 / FRAMES as f32 * std::f32::consts::TAU;
                        ghost(Some(color_for(s)), -BOB * phase.sin())
                    })
                    .collect();
                (s, cycle)
            })
            .collect();
        let Some(resting) = resting_icon() else { return };

        let attention = app.state::<Attention>();
        // What's on screen now: None = resting, Some((state, bobbing)).
        let mut shown: Option<(AgentState, bool)> = None;
        let mut frame = 0usize;
        loop {
            let Some(tray) = app.tray_by_id(tray_id) else {
                thread::sleep(IDLE_POLL);
                continue;
            };
            let want = attention
                .urgent()
                .map(|s| (s, !attention.seen.load(Ordering::SeqCst) && motion(&app)));
            match want {
                None => {
                    if shown.is_some() {
                        let _ = tray.set_icon(Some(resting.clone()));
                        let _ = tray.set_icon_as_template(true);
                        shown = None;
                    }
                    thread::sleep(IDLE_POLL);
                }
                Some((state, bobbing)) => {
                    let cycle = &frames.iter().find(|(s, _)| *s == state).expect("every state has frames").1;
                    if shown.is_none() {
                        let _ = tray.set_icon_as_template(false);
                    }
                    if bobbing {
                        frame = (frame + 1) % cycle.len();
                        let _ = tray.set_icon(Some(cycle[frame].clone()));
                        shown = Some((state, true));
                        thread::sleep(FRAME_TIME);
                    } else {
                        if shown != Some((state, false)) {
                            // Still: the frame at the middle of the bob.
                            let _ = tray.set_icon(Some(cycle[0].clone()));
                            shown = Some((state, false));
                        }
                        thread::sleep(IDLE_POLL);
                    }
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str, state: AgentState) -> Item {
        Item { session_id: id.into(), agent: "claude", cwd: None, folder: "~/p".into(), state, since_ms: 1 }
    }

    #[test]
    fn rings_once_per_session_and_again_when_it_blocks() {
        let a = Attention::default();
        assert!(a.raise(item("s", AgentState::Done)));
        // Finished, then the turn comes back: same session, no second ring.
        assert!(!a.raise(item("s", AgentState::Turn)));
        assert_eq!(a.items()[0].state, AgentState::Turn);
        // Blocked is new news.
        assert!(a.raise(item("s", AgentState::Waiting)));
        assert_eq!(a.items().len(), 1);
    }

    #[test]
    fn color_follows_the_most_urgent_and_a_new_arrival_bobs_again() {
        let a = Attention::default();
        a.raise(item("a", AgentState::Done));
        a.raise(item("b", AgentState::Turn));
        assert_eq!(a.urgent(), Some(AgentState::Turn));
        a.mark_seen();
        assert!(a.seen.load(Ordering::SeqCst));
        a.raise(item("c", AgentState::Done));
        assert!(!a.seen.load(Ordering::SeqCst));
        a.remove("b");
        assert_eq!(a.urgent(), Some(AgentState::Done));
        a.clear();
        assert!(a.is_empty());
    }

    #[test]
    fn draws_the_ghost_in_color_with_its_eyes_cut_out() {
        let img = ghost(Some("#ee0000"), 0.0).expect("renders");
        let px = img.rgba();
        let at = |x: u32, y: u32| &px[((y * ICON_PX + x) * 4) as usize..][..4];
        // Body center is opaque red; a corner is transparent.
        let body = at(ICON_PX / 2, ICON_PX * 3 / 4);
        assert!(body[3] > 200 && body[0] > 200 && body[1] < 40, "body {body:?}");
        assert_eq!(at(0, 0)[3], 0);
    }
}
