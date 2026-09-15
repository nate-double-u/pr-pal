use serde::{Deserialize, Serialize};

use crate::scoring::ScoringConfig;

fn default_refresh_interval() -> u64 {
    300
}

fn default_theme() -> String {
    "auto".to_string()
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Global scoring configuration (applies to all queries unless overridden)
    #[serde(default)]
    pub scoring: Option<ScoringConfig>,

    pub queries: Vec<QueryConfig>,

    /// Auto-refresh interval in seconds (defaults to 300 = 5 minutes)
    #[serde(default = "default_refresh_interval")]
    pub auto_refresh_interval: u64,

    /// Theme selection: "dark", "light", or "auto" (detects terminal background)
    #[serde(default = "default_theme")]
    pub theme: String,

    /// Suppression of reviewed PRs that are awaiting the author
    #[serde(default)]
    pub suppress: Option<SuppressConfig>,
}

/// Suppression policy for PRs the user has reviewed.
///
/// When enabled, reviewed PRs with no notable activity since the user's last
/// review or comment are hidden from the Active list (shown in the Snoozed
/// tab as "awaiting author"). Wake events and a resurface valve bring them
/// back. State is derived fresh each refresh; nothing is persisted.
///
/// Example YAML:
/// ```yaml
/// suppress:
///   awaiting_author: true
///   wake_on: [push, mention, review_request]
///   resurface_after: 21d   # or "never"
/// ```
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SuppressConfig {
    /// Hide awaiting-author PRs from the Active list (default: true)
    #[serde(default = "default_awaiting_author")]
    pub awaiting_author: bool,

    /// Events that resurface a suppressed PR (default: all)
    #[serde(default = "default_wake_on")]
    pub wake_on: Vec<WakeEvent>,

    /// Safety valve: resurface a quiet PR after this long, e.g. "21d";
    /// "never" disables the valve (default: "21d")
    #[serde(default = "default_resurface_after")]
    pub resurface_after: String,
}

/// Events that resurface a suppressed PR.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WakeEvent {
    /// New commits on the PR
    Push,
    /// The user was @-mentioned
    Mention,
    /// The user's review was re-requested
    ReviewRequest,
}

fn default_awaiting_author() -> bool {
    true
}

fn default_wake_on() -> Vec<WakeEvent> {
    vec![
        WakeEvent::Push,
        WakeEvent::Mention,
        WakeEvent::ReviewRequest,
    ]
}

fn default_resurface_after() -> String {
    "21d".to_string()
}

impl SuppressConfig {
    /// Parse `resurface_after` into a chrono Duration; `None` means never.
    pub fn resurface_duration(&self) -> Result<Option<chrono::Duration>, String> {
        let raw = self.resurface_after.trim();
        if raw.eq_ignore_ascii_case("never") {
            return Ok(None);
        }
        let std_duration = humantime::parse_duration(raw).map_err(|e| {
            format!(
                "suppress.resurface_after: invalid duration '{}' - {}",
                raw, e
            )
        })?;
        chrono::Duration::from_std(std_duration)
            .map(Some)
            .map_err(|e| {
                format!(
                    "suppress.resurface_after: duration out of range '{}' - {}",
                    raw, e
                )
            })
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct QueryConfig {
    pub name: Option<String>,
    pub query: String,

    /// Per-query scoring configuration (merges with global scoring — set fields override, unset fields inherit from global)
    #[serde(default)]
    pub scoring: Option<ScoringConfig>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal_yaml(extra: &str) -> String {
        format!(
            "queries:\n  - query: \"review-requested:@me is:open\"\n{}",
            extra
        )
    }

    // LOCKED: regression for since-my-review review workflow (feat/since-my-review).
    // suppress must default to absent (feature off).
    #[test]
    fn suppress_absent_by_default() {
        let config: Config = serde_saphyr::from_str(&minimal_yaml("")).unwrap();
        assert!(config.suppress.is_none());
    }

    // LOCKED: regression for since-my-review review workflow (feat/since-my-review).
    // Explicit suppress fields must round-trip from YAML.
    #[test]
    fn suppress_parses_with_explicit_fields() {
        let yaml = minimal_yaml(
            "suppress:\n  awaiting_author: true\n  wake_on: [push, mention]\n  resurface_after: 30d\n",
        );
        let config: Config = serde_saphyr::from_str(&yaml).unwrap();
        let suppress = config.suppress.expect("suppress should parse");
        assert!(suppress.awaiting_author);
        assert_eq!(suppress.wake_on, vec![WakeEvent::Push, WakeEvent::Mention]);
        assert_eq!(
            suppress.resurface_duration().unwrap(),
            Some(chrono::Duration::days(30))
        );
    }

    // LOCKED: regression for since-my-review review workflow (feat/since-my-review).
    // Bare suppress block: enabled, all wakes, 21d valve.
    #[test]
    fn suppress_defaults_enable_everything() {
        let yaml = minimal_yaml("suppress: {}\n");
        let config: Config = serde_saphyr::from_str(&yaml).unwrap();
        let suppress = config.suppress.expect("suppress should parse");
        assert!(suppress.awaiting_author);
        assert_eq!(
            suppress.wake_on,
            vec![
                WakeEvent::Push,
                WakeEvent::Mention,
                WakeEvent::ReviewRequest
            ]
        );
        assert_eq!(
            suppress.resurface_duration().unwrap(),
            Some(chrono::Duration::days(21))
        );
    }

    // LOCKED: regression for since-my-review review workflow (feat/since-my-review).
    // "never" must disable the resurface valve.
    #[test]
    fn resurface_never_disables_the_valve() {
        let yaml = minimal_yaml("suppress:\n  resurface_after: never\n");
        let config: Config = serde_saphyr::from_str(&yaml).unwrap();
        let suppress = config.suppress.unwrap();
        assert_eq!(suppress.resurface_duration().unwrap(), None);
    }

    // LOCKED: regression for since-my-review review workflow (feat/since-my-review).
    // Unparseable resurface_after must be a config error.
    #[test]
    fn invalid_resurface_duration_is_an_error() {
        let yaml = minimal_yaml("suppress:\n  resurface_after: eleventy\n");
        let config: Config = serde_saphyr::from_str(&yaml).unwrap();
        let err = config.suppress.unwrap().resurface_duration().unwrap_err();
        assert!(err.contains("suppress.resurface_after"));
        assert!(err.contains("eleventy"));
    }

    // LOCKED: regression for since-my-review review workflow (feat/since-my-review).
    // Unknown wake_on values must be rejected.
    #[test]
    fn unknown_wake_event_fails_to_parse() {
        let yaml = minimal_yaml("suppress:\n  wake_on: [carrier_pigeon]\n");
        let result: Result<Config, _> = serde_saphyr::from_str(&yaml);
        assert!(result.is_err());
    }
}
