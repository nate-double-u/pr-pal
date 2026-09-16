use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnoozeState {
    pub version: u32,
    #[serde(default)]
    pub snoozed: HashMap<String, SnoozeEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnoozeEntry {
    pub snoozed_at: DateTime<Utc>,
    /// When the PR wakes. A snooze always wakes; permanent hiding is an
    /// ignore (see `crate::ignore`).
    pub snooze_until: DateTime<Utc>,
}

impl SnoozeEntry {
    /// Format the remaining time until snooze expires in human-friendly form
    /// ("{N}h left" style, or "expired")
    pub fn format_remaining(&self) -> String {
        let now = Utc::now();
        if self.snooze_until <= now {
            return "expired".to_string();
        }
        let duration = self.snooze_until - now;
        let hours = duration.num_hours();
        let days = duration.num_days();
        let weeks = days / 7;

        if weeks >= 1 {
            format!("{}w left", weeks)
        } else if days >= 1 {
            format!("{}d left", days)
        } else if hours >= 1 {
            format!("{}h left", hours)
        } else {
            let minutes = duration.num_minutes();
            if minutes >= 1 {
                format!("{}m left", minutes)
            } else {
                "<1m left".to_string()
            }
        }
    }
}

impl Default for SnoozeState {
    fn default() -> Self {
        Self::new()
    }
}

impl SnoozeState {
    /// Create a new empty snooze state with version 1
    pub fn new() -> Self {
        Self {
            version: 1,
            snoozed: HashMap::new(),
        }
    }

    /// Check if a PR is currently snoozed (not yet expired)
    pub fn is_snoozed(&self, pr_url: &str) -> bool {
        self.snoozed
            .get(pr_url)
            .is_some_and(|entry| Utc::now() < entry.snooze_until)
    }

    /// Snooze a PR until `until`
    pub fn snooze(&mut self, pr_url: String, until: DateTime<Utc>) {
        let entry = SnoozeEntry {
            snoozed_at: Utc::now(),
            snooze_until: until,
        };
        self.snoozed.insert(pr_url, entry);
    }

    /// Remove a PR from snooze state
    /// Returns true if the PR was previously snoozed, false otherwise
    pub fn unsnooze(&mut self, pr_url: &str) -> bool {
        self.snoozed.remove(pr_url).is_some()
    }

    /// Remove expired snooze entries
    pub fn clean_expired(&mut self) {
        let now = Utc::now();
        self.snoozed.retain(|_url, entry| now < entry.snooze_until);
    }

    /// Get a reference to all snoozed entries (for listing snoozed PRs)
    pub fn snoozed_entries(&self) -> &HashMap<String, SnoozeEntry> {
        &self.snoozed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    #[test]
    fn test_new_state_empty() {
        let state = SnoozeState::new();
        assert_eq!(state.version, 1);
        assert!(state.snoozed.is_empty());
    }

    #[test]
    fn test_snooze_with_future_time() {
        let mut state = SnoozeState::new();
        let future = Utc::now() + Duration::hours(1);
        state.snooze("https://github.com/owner/repo/pull/1".to_string(), future);
        assert!(state.is_snoozed("https://github.com/owner/repo/pull/1"));
    }

    #[test]
    fn test_snooze_expired() {
        let mut state = SnoozeState::new();
        let past = Utc::now() - Duration::hours(1);
        state.snooze("https://github.com/owner/repo/pull/1".to_string(), past);
        assert!(!state.is_snoozed("https://github.com/owner/repo/pull/1"));
    }

    #[test]
    fn test_unsnooze() {
        let mut state = SnoozeState::new();
        let future = Utc::now() + Duration::hours(1);
        state.snooze("https://github.com/owner/repo/pull/1".to_string(), future);
        assert!(state.unsnooze("https://github.com/owner/repo/pull/1"));
        assert!(!state.is_snoozed("https://github.com/owner/repo/pull/1"));
    }

    #[test]
    fn test_unsnooze_missing() {
        let mut state = SnoozeState::new();
        assert!(!state.unsnooze("https://github.com/owner/repo/pull/1"));
    }

    #[test]
    fn test_clean_expired() {
        let mut state = SnoozeState::new();

        // Future snooze (should be kept)
        let future = Utc::now() + Duration::hours(1);
        state.snooze("https://github.com/owner/repo/pull/2".to_string(), future);

        // Expired snooze (should be removed)
        let past = Utc::now() - Duration::hours(1);
        state.snooze("https://github.com/owner/repo/pull/3".to_string(), past);

        assert_eq!(state.snoozed.len(), 2);

        state.clean_expired();

        assert_eq!(state.snoozed.len(), 1);
        assert!(state.is_snoozed("https://github.com/owner/repo/pull/2"));
        assert!(!state.is_snoozed("https://github.com/owner/repo/pull/3"));
    }

    #[test]
    fn test_format_remaining_future_hours() {
        let future = Utc::now() + Duration::hours(3);
        let entry = SnoozeEntry {
            snoozed_at: Utc::now(),
            snooze_until: future,
        };
        let result = entry.format_remaining();
        // Should be something like "2h left" or "3h left" (timing may vary slightly)
        assert!(
            result.ends_with("h left"),
            "Expected hours format, got: {}",
            result
        );
    }

    #[test]
    fn test_format_remaining_expired() {
        let past = Utc::now() - Duration::hours(1);
        let entry = SnoozeEntry {
            snoozed_at: Utc::now() - Duration::hours(2),
            snooze_until: past,
        };
        assert_eq!(entry.format_remaining(), "expired");
    }

    #[test]
    fn test_format_remaining_days() {
        let future = Utc::now() + Duration::days(3);
        let entry = SnoozeEntry {
            snoozed_at: Utc::now(),
            snooze_until: future,
        };
        let result = entry.format_remaining();
        assert!(
            result.ends_with("d left"),
            "Expected days format, got: {}",
            result
        );
    }

    #[test]
    fn test_format_remaining_weeks() {
        let future = Utc::now() + Duration::weeks(2);
        let entry = SnoozeEntry {
            snoozed_at: Utc::now(),
            snooze_until: future,
        };
        let result = entry.format_remaining();
        assert!(
            result.ends_with("w left"),
            "Expected weeks format, got: {}",
            result
        );
    }
}
