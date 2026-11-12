use std::time::SystemTime;

use serde_json::Value;

use crate::dates::{from_unix_secs, parse_iso8601};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Provider {
    Codex,
    Claude,
}

impl Provider {
    pub const ALL: [Provider; 2] = [Provider::Codex, Provider::Claude];

    pub fn name(self) -> &'static str {
        match self {
            Self::Codex => "Codex",
            Self::Claude => "Claude Code",
        }
    }

    pub fn login_command(self) -> &'static str {
        match self {
            Self::Codex => "codex login",
            Self::Claude => "claude auth login",
        }
    }

    pub fn endpoint(self) -> &'static str {
        match self {
            Self::Codex => "https://chatgpt.com/backend-api/wham/usage",
            Self::Claude => "https://api.anthropic.com/api/oauth/usage",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuotaError {
    NotSignedIn,
    InvalidCredentials,
    ApiKeyOnly,
    Expired,
    PermissionRequired,
    Forbidden,
    Unavailable,
    Network,
    RateLimited { until: SystemTime },
}

impl QuotaError {
    pub fn message(&self, provider: Provider) -> String {
        let (name, login) = (provider.name(), provider.login_command());
        match self {
            Self::NotSignedIn => format!("No local login found. Run {login} in your terminal, then refresh."),
            Self::InvalidCredentials => format!("Couldn't read the local login. Sign in with {login}, then refresh."),
            Self::ApiKeyOnly => "Subscription quota isn't available for API-key logins.".to_string(),
            Self::Expired => format!("The local login has expired. Open {name} to renew it, then refresh."),
            Self::PermissionRequired => format!("Allow Keychain access to read your existing {name} login."),
            Self::Forbidden => "This login can't access subscription quota. Check your subscription and sign in again.".to_string(),
            Self::Unavailable => "Quota isn't available in this response. Try refreshing later.".to_string(),
            Self::Network => format!("Couldn't reach {name}. Check your connection, then refresh."),
            Self::RateLimited { .. } => "Too many quota requests. Refresh will be available after the cooldown.".to_string(),
        }
    }
}

impl std::fmt::Display for QuotaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for QuotaError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowKind {
    FiveHour,
    Weekly,
}

impl WindowKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::FiveHour => "5-hour",
            Self::Weekly => "Weekly",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct QuotaWindow {
    pub kind: WindowKind,
    pub remaining_percent: f64,
    pub resets_at: Option<SystemTime>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct QuotaSnapshot {
    pub five_hour: Option<QuotaWindow>,
    pub weekly: Option<QuotaWindow>,
}

fn number(value: Option<&Value>) -> Option<f64> {
    value?.as_f64().filter(|n| n.is_finite())
}

impl QuotaSnapshot {
    /// The window the usage ring shows: the 5-hour one, even when depleted, else weekly.
    pub fn ring_window(&self) -> Option<&QuotaWindow> {
        self.five_hour.as_ref().or(self.weekly.as_ref())
    }

    pub fn decode(data: &[u8], provider: Provider) -> Result<Self, QuotaError> {
        let root: Value = serde_json::from_slice(data).map_err(|_| QuotaError::Unavailable)?;
        let root = root.as_object().ok_or(QuotaError::Unavailable)?;
        let (mut five_hour, mut weekly) = (None, None);
        match provider {
            Provider::Codex => {
                let limits = root.get("rate_limit").and_then(Value::as_object);
                for key in ["primary_window", "secondary_window"] {
                    let Some(raw) = limits.and_then(|l| l.get(key)).and_then(Value::as_object) else { continue };
                    let (Some(duration), Some(used)) =
                        (number(raw.get("limit_window_seconds")), number(raw.get("used_percent")))
                    else {
                        continue;
                    };
                    if used < 0.0 {
                        continue;
                    }
                    let resets_at = number(raw.get("reset_at")).filter(|r| *r > 0.0).and_then(from_unix_secs);
                    let window = |kind| QuotaWindow { kind, remaining_percent: (100.0 - used).max(0.0), resets_at };
                    if duration == 18_000.0 {
                        five_hour = Some(window(WindowKind::FiveHour));
                    } else if duration == 604_800.0 {
                        weekly = Some(window(WindowKind::Weekly));
                    }
                }
            }
            Provider::Claude => {
                let window = |key: &str, kind| {
                    let raw = root.get(key)?.as_object()?;
                    let used = number(raw.get("utilization")).filter(|u| *u >= 0.0)?;
                    let resets_at = raw.get("resets_at").and_then(Value::as_str).and_then(parse_iso8601);
                    Some(QuotaWindow { kind, remaining_percent: (100.0 - used).max(0.0), resets_at })
                };
                five_hour = window("five_hour", WindowKind::FiveHour);
                weekly = window("seven_day", WindowKind::Weekly);
            }
        }
        if five_hour.is_none() && weekly.is_none() {
            return Err(QuotaError::Unavailable);
        }
        Ok(Self { five_hour, weekly })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, UNIX_EPOCH};

    fn decode(json: &str, provider: Provider) -> Result<QuotaSnapshot, QuotaError> {
        QuotaSnapshot::decode(json.as_bytes(), provider)
    }

    #[test]
    fn codex_classifies_windows_by_duration_and_shows_remaining() {
        let snapshot = decode(
            r#"{"rate_limit":{"primary_window":{"used_percent":30,"limit_window_seconds":604800,"reset_at":1800000000},
            "secondary_window":{"used_percent":80,"limit_window_seconds":18000,"reset_at":1799990000}}}"#,
            Provider::Codex,
        )
        .unwrap();
        assert_eq!(snapshot.five_hour.unwrap().remaining_percent, 20.0);
        assert_eq!(snapshot.weekly.unwrap().remaining_percent, 70.0);
        assert_eq!(snapshot.ring_window().unwrap().kind, WindowKind::FiveHour);
        assert_eq!(snapshot.five_hour.unwrap().resets_at, Some(UNIX_EPOCH + Duration::from_secs(1_799_990_000)));
    }

    #[test]
    fn depleted_five_hour_does_not_fall_back_to_weekly() {
        let snapshot = decode(r#"{"five_hour":{"utilization":100,"resets_at":null},"seven_day":{"utilization":25}}"#, Provider::Claude).unwrap();
        assert_eq!(snapshot.ring_window().unwrap().kind, WindowKind::FiveHour);
        assert_eq!(snapshot.ring_window().unwrap().remaining_percent, 0.0);
    }

    #[test]
    fn absent_or_invalid_five_hour_uses_weekly() {
        for window in ["null", "{}", r#"{"utilization":-1}"#, r#"{"utilization":"bad"}"#] {
            let snapshot = decode(
                &format!(r#"{{"five_hour":{window},"seven_day":{{"utilization":12.5,"resets_at":"2026-11-10T12:30:00.123Z"}}}}"#),
                Provider::Claude,
            )
            .unwrap();
            assert!(snapshot.five_hour.is_none(), "{window}");
            assert_eq!(snapshot.ring_window().unwrap().kind, WindowKind::Weekly);
            assert_eq!(snapshot.ring_window().unwrap().remaining_percent, 87.5);
            assert!(snapshot.weekly.unwrap().resets_at.is_some());
        }
    }

    #[test]
    fn invalid_reset_date_does_not_discard_quota_and_overage_is_empty() {
        let snapshot = decode(r#"{"five_hour":{"utilization":125,"resets_at":"invalid"}}"#, Provider::Claude).unwrap();
        assert_eq!(snapshot.ring_window().unwrap().remaining_percent, 0.0);
        assert_eq!(snapshot.ring_window().unwrap().resets_at, None);
    }

    #[test]
    fn missing_quota_is_unavailable() {
        for json in ["{}", r#"{"five_hour":null,"seven_day":null}"#, "[]", "not json"] {
            assert_eq!(decode(json, Provider::Claude), Err(QuotaError::Unavailable), "{json}");
        }
    }

    #[test]
    fn unrecognized_codex_duration_is_not_mislabeled() {
        let json = r#"{"rate_limit":{"primary_window":{"used_percent":1,"limit_window_seconds":3600}}}"#;
        assert_eq!(decode(json, Provider::Codex), Err(QuotaError::Unavailable));
    }

    #[test]
    fn booleans_are_not_numbers() {
        let json = r#"{"five_hour":{"utilization":true}}"#;
        assert_eq!(decode(json, Provider::Claude), Err(QuotaError::Unavailable));
    }

    #[test]
    fn messages_name_the_provider() {
        assert!(QuotaError::NotSignedIn.message(Provider::Codex).contains("codex login"));
        assert!(QuotaError::Expired.message(Provider::Claude).contains("Claude Code"));
    }
}
