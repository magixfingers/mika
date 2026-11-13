//! User settings, stored as JSON in Mika's app config folder, plus the Claude
//! Code hooks installer that the Integrations section uses.

use std::fs;
use std::path::{Path, PathBuf};

use chrono::{Local, NaiveTime};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// macOS system sounds the alert can use (files in /System/Library/Sounds).
pub const SOUNDS: [&str; 14] = [
    "Basso", "Blow", "Bottle", "Frog", "Funk", "Glass", "Hero", "Morse", "Ping", "Pop", "Purr", "Sosumi", "Submarine",
    "Tink",
];

/// The Liquid Glass skin's look, as 0–100 sliders (frost is a blur in pixels).
/// The defaults reproduce the original hand-tuned look.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Glass {
    /// How much of the backdrop shows through the glass tint.
    pub transparency: u8,
    /// Backdrop blur in CSS pixels, 0–40.
    pub frost: u8,
    /// Width of the bent edge.
    pub refraction: u8,
    /// Strength of the white edge highlight.
    pub shine: u8,
    pub shadow: u8,
    /// The colours behind the glass: "pastel", "mono", "ocean" or "sunset".
    pub backdrop: String,
}

pub const BACKDROPS: [&str; 4] = ["pastel", "mono", "ocean", "sunset"];

impl Default for Glass {
    fn default() -> Self {
        Self { transparency: 45, frost: 18, refraction: 50, shine: 70, shadow: 50, backdrop: "pastel".into() }
    }
}

impl Glass {
    fn sanitized(mut self) -> Self {
        for value in [&mut self.transparency, &mut self.refraction, &mut self.shine, &mut self.shadow] {
            *value = (*value).min(100);
        }
        self.frost = self.frost.min(40);
        if !BACKDROPS.contains(&self.backdrop.as_str()) {
            self.backdrop = "pastel".into();
        }
        self
    }
}

/// Every field has a default, so older or partial settings files still load.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Settings {
    // Alerts
    pub sound_enabled: bool,
    pub sound: String,
    pub alert_on_done: bool,
    /// When an agent finishes and waits for the next message: "off" (not
    /// shown), "status" (a calm "Your turn", no sound) or "chime" (also one sound).
    pub turn_alert: String,
    /// Pop up a window with a repeating ringtone when an agent needs you.
    pub alert_popup: bool,
    /// Answer permission requests (Allow, Always allow, Deny) in the popup.
    pub answer_from_mika: bool,
    pub quiet_hours: bool,
    /// Local time, `HH:MM`.
    pub quiet_start: String,
    pub quiet_end: String,
    // Display
    /// "day", "month" or "total".
    pub default_range: String,
    pub show_cost: bool,
    /// "full", "subtle" or "off".
    pub motion: String,
    /// "full", "compact" or "icon".
    pub tray_title: String,
    /// "classic" (flat, CSS only) or "liquid" (WebGPU glass over a wallpaper).
    pub skin: String,
    pub glass: Glass,
    // Data and privacy
    pub track_claude: bool,
    pub track_codex: bool,
    pub claude_limits: bool,
    pub sample_interval_ms: u64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            sound_enabled: true,
            sound: "Glass".into(),
            alert_on_done: false,
            turn_alert: "status".into(),
            alert_popup: true,
            answer_from_mika: true,
            quiet_hours: false,
            quiet_start: "22:00".into(),
            quiet_end: "08:00".into(),
            default_range: "day".into(),
            show_cost: true,
            motion: "full".into(),
            tray_title: "full".into(),
            skin: "classic".into(),
            glass: Glass::default(),
            track_claude: true,
            track_codex: true,
            claude_limits: true,
            sample_interval_ms: 1000,
        }
    }
}

impl Settings {
    /// Clamps values the UI could send out of range.
    pub fn sanitized(mut self) -> Self {
        if !SOUNDS.contains(&self.sound.as_str()) {
            self.sound = "Glass".into();
        }
        if !["off", "status", "chime"].contains(&self.turn_alert.as_str()) {
            self.turn_alert = "status".into();
        }
        if !["classic", "liquid"].contains(&self.skin.as_str()) {
            self.skin = "classic".into();
        }
        self.sample_interval_ms = self.sample_interval_ms.clamp(500, 10_000);
        self.glass = self.glass.sanitized();
        self
    }

    pub fn sound_path(&self) -> String {
        format!("/System/Library/Sounds/{}.aiff", self.sound)
    }

    /// True when quiet hours are on and `now` falls inside them (spans midnight if start > end).
    pub fn is_quiet(&self, now: NaiveTime) -> bool {
        let parse = |s: &str| NaiveTime::parse_from_str(s, "%H:%M").ok();
        let (Some(start), Some(end)) = (parse(&self.quiet_start), parse(&self.quiet_end)) else {
            return false;
        };
        self.quiet_hours
            && if start <= end {
                now >= start && now < end
            } else {
                now >= start || now < end
            }
    }

    pub fn is_quiet_now(&self) -> bool {
        self.is_quiet(Local::now().time())
    }
}

pub fn load(path: &Path) -> Settings {
    fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str::<Settings>(&s).ok())
        .unwrap_or_default()
        .sanitized()
}

pub fn save(path: &Path, settings: &Settings) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    fs::write(path, json).map_err(|e| e.to_string())
}

// --- Claude Code hooks -------------------------------------------------------

const HOOK_MARKER: &str = "127.0.0.1:47321";

/// An agent whose hooks file Mika can add itself to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Integration {
    Claude,
    Codex,
}

impl Integration {
    /// The file the agent reads hooks from.
    pub fn hooks_path(self, home: &Path) -> PathBuf {
        match self {
            Integration::Claude => home.join(".claude/settings.json"),
            Integration::Codex => home.join(".codex/hooks.json"),
        }
    }

    /// Mika's hook entries, in the agent's own format.
    fn template(self) -> &'static str {
        match self {
            Integration::Claude => include_str!("../../hooks/claude-code-settings.json"),
            Integration::Codex => include_str!("../../hooks/codex-hooks.json"),
        }
    }
}

fn is_mika_group(group: &Value) -> bool {
    group.to_string().contains(HOOK_MARKER)
}

/// Whether every hook event Mika needs has Mika's current entry. An older
/// Mika entry counts as missing, so the Integrations section offers to update it.
pub fn hooks_installed(integration: Integration, settings: &Value) -> bool {
    let ours: Value = serde_json::from_str(integration.template()).unwrap_or_default();
    let Some(events) = ours["hooks"].as_object() else { return false };
    events.iter().all(|(event, groups)| {
        let wanted = groups.as_array().cloned().unwrap_or_default();
        settings["hooks"][event]
            .as_array()
            .is_some_and(|have| wanted.iter().all(|g| have.contains(g)))
    })
}

/// Whether some Mika hook entry is present, current or not.
pub fn any_mika_hooks(settings: &Value) -> bool {
    settings["hooks"].as_object().is_some_and(|events| {
        events.values().filter_map(Value::as_array).flatten().any(is_mika_group)
    })
}

/// Adds Mika's hooks next to any existing ones, replacing older Mika entries.
pub fn with_hooks(integration: Integration, mut settings: Value) -> Value {
    let ours: Value = serde_json::from_str(integration.template()).unwrap_or_default();
    if !settings.is_object() {
        settings = Value::Object(Default::default());
    }
    let hooks = settings
        .as_object_mut()
        .unwrap()
        .entry("hooks")
        .or_insert_with(|| Value::Object(Default::default()));
    if let (Some(hooks), Some(events)) = (hooks.as_object_mut(), ours["hooks"].as_object()) {
        for (event, groups) in events {
            let list = hooks.entry(event.clone()).or_insert_with(|| Value::Array(vec![]));
            if let Some(list) = list.as_array_mut() {
                list.retain(|group| !is_mika_group(group));
                list.extend(groups.as_array().cloned().unwrap_or_default());
            }
        }
    }
    settings
}

/// Removes only Mika's hook entries, leaving the user's own hooks alone.
pub fn without_hooks(mut settings: Value) -> Value {
    if let Some(hooks) = settings["hooks"].as_object_mut() {
        for list in hooks.values_mut() {
            if let Some(list) = list.as_array_mut() {
                list.retain(|group| !is_mika_group(group));
            }
        }
        hooks.retain(|_, list| list.as_array().is_none_or(|l| !l.is_empty()));
    }
    settings
}

pub fn read_hooks_file(integration: Integration, home: &Path) -> Value {
    fs::read_to_string(integration.hooks_path(home))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or(Value::Null)
}

/// Rewrites the agent's hooks file after backing up the current one.
pub fn update_hooks_file(
    integration: Integration,
    home: &Path,
    change: impl FnOnce(Value) -> Value,
) -> Result<(), String> {
    let path = integration.hooks_path(home);
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let current = fs::read_to_string(&path).unwrap_or_else(|_| "{}".into());
    let parsed: Value = serde_json::from_str(&current).map_err(|e| format!("{name} isn't valid JSON: {e}"))?;
    if path.exists() {
        let stamp = Local::now().format("%Y%m%d-%H%M%S");
        fs::write(path.with_file_name(format!("{name}.bak-mika-{stamp}")), &current).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(&change(parsed)).map_err(|e| e.to_string())?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    fs::write(&path, json + "\n").map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quiet_hours_span_midnight() {
        let s = Settings { quiet_hours: true, ..Settings::default() };
        let t = |h, m| NaiveTime::from_hms_opt(h, m, 0).unwrap();
        assert!(s.is_quiet(t(23, 0)));
        assert!(s.is_quiet(t(7, 59)));
        assert!(!s.is_quiet(t(8, 0)));
        assert!(!s.is_quiet(t(12, 0)));
        assert!(!Settings::default().is_quiet(t(23, 0)), "off by default");
    }

    #[test]
    fn hooks_install_and_remove_without_touching_others() {
        let user = serde_json::json!({
            "model": "opus",
            "hooks": {"Stop": [{"hooks": [{"type": "command", "command": "say done"}]}]}
        });
        for integration in [Integration::Claude, Integration::Codex] {
            assert!(!hooks_installed(integration, &user));
            let installed = with_hooks(integration, user.clone());
            assert!(hooks_installed(integration, &installed));
            // Installing twice doesn't duplicate.
            assert_eq!(with_hooks(integration, installed.clone()), installed);
            let removed = without_hooks(installed);
            assert!(!hooks_installed(integration, &removed));
            assert_eq!(removed, user);
        }
    }

    #[test]
    fn hooks_upgrade_replaces_an_older_mika_entry() {
        let old = |url: &str| serde_json::json!([{"hooks": [{"type": "command", "command": format!("curl -s -m 1 {url} || true")}]}]);
        let mut outdated = with_hooks(Integration::Codex, serde_json::json!({}));
        outdated["hooks"]["PermissionRequest"] = old("http://127.0.0.1:47321/hook/codex");
        assert!(!hooks_installed(Integration::Codex, &outdated));
        let upgraded = with_hooks(Integration::Codex, outdated);
        assert!(hooks_installed(Integration::Codex, &upgraded));
        let groups = upgraded["hooks"]["PermissionRequest"].as_array().unwrap();
        assert_eq!(groups.len(), 1);
        assert!(groups[0].to_string().contains("/permission/codex"));
    }

    #[test]
    fn partial_files_and_bad_values_fall_back() {
        let s: Settings = serde_json::from_str(r#"{"sound": "Nope", "sample_interval_ms": 1}"#).unwrap();
        let s = s.sanitized();
        assert_eq!(s.sound, "Glass");
        assert_eq!(s.sample_interval_ms, 500);
        assert!(s.sound_enabled);
        assert_eq!(s.glass, Glass::default());
    }

    #[test]
    fn glass_values_are_clamped() {
        let s: Settings =
            serde_json::from_str(r#"{"glass": {"transparency": 250, "frost": 99, "backdrop": "neon"}}"#).unwrap();
        let g = s.sanitized().glass;
        assert_eq!((g.transparency, g.frost, g.backdrop.as_str()), (100, 40, "pastel"));
        assert_eq!(g.shine, Glass::default().shine);
    }
}
