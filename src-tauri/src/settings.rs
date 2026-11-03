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

/// Every field has a default, so older or partial settings files still load.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Settings {
    // Alerts
    pub sound_enabled: bool,
    pub sound: String,
    pub dock_bounce: bool,
    pub alert_on_done: bool,
    /// Pop up a window with a repeating ringtone when an agent needs you.
    pub alert_popup: bool,
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
            dock_bounce: true,
            alert_on_done: false,
            alert_popup: true,
            quiet_hours: false,
            quiet_start: "22:00".into(),
            quiet_end: "08:00".into(),
            default_range: "day".into(),
            show_cost: true,
            motion: "full".into(),
            tray_title: "full".into(),
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
        self.sample_interval_ms = self.sample_interval_ms.clamp(500, 10_000);
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

/// Whether every hook event Mika needs has a Mika entry.
pub fn hooks_installed(integration: Integration, settings: &Value) -> bool {
    let ours: Value = serde_json::from_str(integration.template()).unwrap_or_default();
    let Some(events) = ours["hooks"].as_object() else { return false };
    events.keys().all(|event| {
        settings["hooks"][event]
            .as_array()
            .is_some_and(|groups| groups.iter().any(is_mika_group))
    })
}

/// Adds Mika's hooks next to any existing ones, without duplicating.
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
            if let Some(list) = list.as_array_mut()
                && !list.iter().any(is_mika_group)
            {
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
    fn partial_files_and_bad_values_fall_back() {
        let s: Settings = serde_json::from_str(r#"{"sound": "Nope", "sample_interval_ms": 1}"#).unwrap();
        let s = s.sanitized();
        assert_eq!(s.sound, "Glass");
        assert_eq!(s.sample_interval_ms, 500);
        assert!(s.sound_enabled);
    }
}
