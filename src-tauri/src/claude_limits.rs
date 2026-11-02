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
        plan: plan_name(oauth),
        windows: parse_windows(&body),
    })
}

/// "max" plus tier "default_claude_max_20x" reads as "Max 20x".
fn plan_name(oauth: &Value) -> Option<String> {
    let plan = oauth["subscriptionType"].as_str()?;
    let multiplier = oauth["rateLimitTier"]
        .as_str()
        .and_then(|t| t.rsplit('_').next())
        .filter(|m| m.ends_with('x') && m[..m.len() - 1].chars().all(|c| c.is_ascii_digit()));
    Some(match multiplier {
        Some(m) => format!("{plan} {m}"),
        None => plan.to_string(),
    })
}

/// Reads the `limits` list: one entry per plan limit with a percent used,
/// including inactive scoped ones (Claude Code's /usage shows those too).
/// Other top-level keys are codenamed experiments, dollar caps or nulls.
fn parse_windows(body: &Value) -> Vec<LimitWindow> {
    let Some(limits) = body["limits"].as_array() else { return Vec::new() };
    limits
        .iter()
        .filter_map(|l| {
            let kind = l["kind"].as_str()?;
            // e.g. {"model": {"display_name": "Fable"}, "surface": null}
            let scope = l["scope"]["model"]["display_name"]
                .as_str()
                .or_else(|| l["scope"]["surface"]["display_name"].as_str())
                .or_else(|| l["scope"].as_str());
            Some(LimitWindow {
                id: match scope {
                    Some(scope) if kind != "weekly_all" => format!("{kind}:{scope}"),
                    _ => kind.to_string(),
                },
                used_percent: l["percent"].as_f64()?,
                resets_at_ms: l["resets_at"]
                    .as_str()
                    .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                    .map(|d| d.timestamp_millis()),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_limits_list() {
        let body = serde_json::json!({
            "iguana_necktie": {"limit_dollars": 250, "remaining_dollars": 221.3},
            "seven_day": {"utilization": 24.0},
            "limits": [
                {"group": "weekly", "is_active": true, "kind": "weekly_all", "percent": 24, "resets_at": "2026-11-08T23:00:00.815787+00:00"},
                {"group": "weekly", "is_active": false, "kind": "weekly_scoped", "percent": 0,
                 "scope": {"model": {"display_name": "Fable", "id": null}, "surface": null}}
            ]
        });
        let w = parse_windows(&body);
        assert_eq!(w.iter().map(|w| w.id.as_str()).collect::<Vec<_>>(), ["weekly_all", "weekly_scoped:Fable"]);
        assert_eq!(w[0].used_percent, 24.0);
        assert!(w[0].resets_at_ms.is_some());
    }

    #[test]
    fn names_the_plan_with_its_tier() {
        let oauth = serde_json::json!({"subscriptionType": "max", "rateLimitTier": "default_claude_max_20x"});
        assert_eq!(plan_name(&oauth).as_deref(), Some("max 20x"));
    }
}
