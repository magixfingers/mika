mod sampler;

use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use sampler::{Sampler, Snapshot};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, State};

const SAMPLE_INTERVAL: Duration = Duration::from_secs(2);
const TRAY_ID: &str = "mika-tray";

/// The latest snapshot, so a window that opens between samples has data at once.
#[derive(Default)]
struct Latest(Mutex<Option<Snapshot>>);

#[tauri::command]
fn latest_snapshot(latest: State<'_, Latest>) -> Option<Snapshot> {
    latest.0.lock().ok().and_then(|s| s.clone())
}

fn tray_title(snapshot: &Snapshot) -> String {
    if snapshot.agents.is_empty() {
        return "Mika".into();
    }
    let cpu: f32 = snapshot.agents.iter().map(|a| a.cpu).sum();
    let mem: u64 = snapshot.agents.iter().map(|a| a.memory).sum();
    format!(
        "{} agent{} · {:.0}% · {:.1} GB",
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
            if let Some(tray) = app.tray_by_id(TRAY_ID) {
                let _ = tray.set_title(Some(tray_title(&snapshot)));
            }
            let _ = app.emit("agents", &snapshot);
            if let Ok(mut latest) = app.state::<Latest>().0.lock() {
                *latest = Some(snapshot);
            }
            thread::sleep(SAMPLE_INTERVAL);
        }
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(Latest::default())
        .invoke_handler(tauri::generate_handler![latest_snapshot])
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
