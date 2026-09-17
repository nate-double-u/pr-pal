use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// PRs hidden permanently. Unlike a snooze, an ignore never wakes; the PR
/// only comes back when the user un-ignores it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IgnoreState {
    pub version: u32,
    #[serde(default)]
    pub ignored: HashMap<String, IgnoreEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IgnoreEntry {
    pub ignored_at: DateTime<Utc>,
}

impl IgnoreEntry {
    /// How long ago the PR was ignored, e.g. "3d ago"; "just now" under a minute.
    pub fn format_age(&self) -> String {
        let age = crate::output::formatter::format_age(Utc::now() - self.ignored_at);
        if age == "now" {
            "just now".to_string()
        } else {
            format!("{} ago", age)
        }
    }
}

impl Default for IgnoreState {
    fn default() -> Self {
        Self::new()
    }
}

impl IgnoreState {
    pub fn new() -> Self {
        Self {
            version: 1,
            ignored: HashMap::new(),
        }
    }

    pub fn is_ignored(&self, pr_url: &str) -> bool {
        self.ignored.contains_key(pr_url)
    }

    /// Ignore a PR. `at` is the ignore timestamp; callers pass `Utc::now()`
    /// for a fresh ignore and the original timestamp when restoring one.
    pub fn ignore(&mut self, pr_url: String, at: DateTime<Utc>) {
        self.ignored.insert(pr_url, IgnoreEntry { ignored_at: at });
    }

    /// Returns true if the PR was previously ignored.
    pub fn unignore(&mut self, pr_url: &str) -> bool {
        self.ignored.remove(pr_url).is_some()
    }

    pub fn ignored_entries(&self) -> &HashMap<String, IgnoreEntry> {
        &self.ignored
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    const URL: &str = "https://github.com/owner/repo/pull/1";

    #[test]
    fn new_state_is_empty_v1() {
        let state = IgnoreState::new();
        assert_eq!(state.version, 1);
        assert!(state.ignored.is_empty());
    }

    #[test]
    fn ignore_marks_url_ignored() {
        let mut state = IgnoreState::new();
        state.ignore(URL.to_string(), Utc::now());
        assert!(state.is_ignored(URL));
        assert!(!state.is_ignored("https://github.com/owner/repo/pull/2"));
    }

    #[test]
    fn ignore_records_given_timestamp() {
        let mut state = IgnoreState::new();
        let at = Utc::now() - Duration::days(3);
        state.ignore(URL.to_string(), at);
        assert_eq!(state.ignored_entries()[URL].ignored_at, at);
    }

    #[test]
    fn unignore_removes_and_reports() {
        let mut state = IgnoreState::new();
        state.ignore(URL.to_string(), Utc::now());
        assert!(state.unignore(URL));
        assert!(!state.is_ignored(URL));
        assert!(!state.unignore(URL), "second unignore finds nothing");
    }

    #[test]
    fn format_age_reads_as_time_since_ignore() {
        let entry = IgnoreEntry {
            ignored_at: Utc::now() - Duration::days(3),
        };
        assert_eq!(entry.format_age(), "3d ago");

        let fresh = IgnoreEntry {
            ignored_at: Utc::now(),
        };
        assert_eq!(fresh.format_age(), "just now");
    }
}
