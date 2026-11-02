//! Fetches Claude plan usage limits the way Claude Code does: with the OAuth
//! sign-in Claude Code keeps in the macOS Keychain.
//!
//! The usage endpoint is undocumented, so parsing is lenient: any top-level
//! object with a `utilization` field is treated as a limit window. The token
//! is only held in memory for the request and never logged.

use std::process::Command;
use std::time::Duration;

use chrono::DateTime;
use serde::Serialize;
use serde_json::Value;

const KEYCHAIN_SERVICE: &str = "Claude Code-credentials";
const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Serialize)]
pub struct LimitWindow {
    /// The endpoint's key, e.g. `five_hour`, `seven_day`, `seven_day_opus`.
    pub id: String,
    pub used_percent: f64,
    pub resets_at_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ClaudeLimits {
    pub plan: Option<String>,
    pub windows: Vec<LimitWindow>,
}

pub fn fetch() -> Result<ClaudeLimits, String> {
    let output = Command::new("security")
        .args(["find-generic-password", "-s", KEYCHAIN_SERVICE, "-w"])
        .output()
        .map_err(|e| format!("could not run security: {e}"))?;
    if !output.status.success() {
        return Err("no Claude Code sign-in in the Keychain, or access was denied".into());
    }
    let creds: Value = serde_json::from_slice(&output.stdout).map_err(|_| "unreadable Claude Code credentials")?;
    let oauth = &creds["claudeAiOauth"];
    let token = oauth["accessToken"].as_str().ok_or("Claude Code credentials have no access token")?;

    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(REQUEST_TIMEOUT))
        .build()
        .into();
    let body: Value = agent
        .get(USAGE_URL)
        .header("Authorization", &format!("Bearer {token}"))
        .header("anthropic-beta", "oauth-2025-04-20")
        .call()
        .map_err(|e| format!("usage request failed: {e}"))?
        .body_mut()
        .read_json()
        .map_err(|e| format!("usage response unreadable: {e}"))?;

    Ok(ClaudeLimits {
        plan: oauth["subscriptionType"].as_str().map(String::from),
        windows: parse_windows(&body),
    })
}

fn parse_windows(body: &Value) -> Vec<LimitWindow> {
    let Some(map) = body.as_object() else { return Vec::new() };
    let mut windows: Vec<LimitWindow> = map
        .iter()
        .filter_map(|(id, w)| {
            let used = w.get("utilization")?.as_f64()?;
            Some(LimitWindow {
                id: id.clone(),
                used_percent: used,
                resets_at_ms: w["resets_at"]
                    .as_str()
                    .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                    .map(|d| d.timestamp_millis()),
            })
        })
        .collect();
    // Shortest window first: five_hour, then seven_day variants.
    windows.sort_by_key(|w| (!w.id.starts_with("five"), w.id.clone()));
    windows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_known_and_unknown_windows() {
        let body = serde_json::json!({
            "seven_day": {"utilization": 23.0, "resets_at": "2026-11-09T07:00:00+00:00"},
            "five_hour": {"utilization": 4.5, "resets_at": null},
            "seven_day_opus": null,
            "extra_usage": {"is_enabled": false}
        });
        let w = parse_windows(&body);
        assert_eq!(w.iter().map(|w| w.id.as_str()).collect::<Vec<_>>(), ["five_hour", "seven_day"]);
        assert_eq!(w[0].used_percent, 4.5);
        assert!(w[0].resets_at_ms.is_none());
        assert!(w[1].resets_at_ms.is_some());
    }
}
