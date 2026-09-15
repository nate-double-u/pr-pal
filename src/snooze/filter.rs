use super::types::SnoozeState;
use crate::github::types::PullRequest;

/// Filter out snoozed PRs, returning only active (non-snoozed) PRs
pub fn filter_active_prs(prs: Vec<PullRequest>, snooze_state: &SnoozeState) -> Vec<PullRequest> {
    prs.into_iter()
        .filter(|pr| !snooze_state.is_snoozed(&pr.url))
        .collect()
}

/// Filter to only snoozed PRs, removing active ones
pub fn filter_snoozed_prs(prs: Vec<PullRequest>, snooze_state: &SnoozeState) -> Vec<PullRequest> {
    prs.into_iter()
        .filter(|pr| snooze_state.is_snoozed(&pr.url))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, Utc};

    fn create_test_pr(number: u64, url: &str) -> PullRequest {
        PullRequest {
            title: format!("PR #{}", number),
            number,
            author: "test-author".to_string(),
            repo: "owner/repo".to_string(),
            url: url.to_string(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            additions: 10,
            deletions: 5,
            approvals: 0,
            draft: false,
            labels: vec![],
            user_has_reviewed: false,
            filtered_size: None,
            signals: Default::default(),
        }
    }

    #[test]
    fn test_filter_active_removes_snoozed() {
        let mut state = SnoozeState::new();
        state.snooze("https://github.com/owner/repo/pull/1".to_string(), None);

        let prs = vec![
            create_test_pr(1, "https://github.com/owner/repo/pull/1"),
            create_test_pr(2, "https://github.com/owner/repo/pull/2"),
        ];

        let active = filter_active_prs(prs, &state);
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].number, 2);
    }

    #[test]
    fn test_filter_active_keeps_unsnoozed() {
        let state = SnoozeState::new();

        let prs = vec![
            create_test_pr(1, "https://github.com/owner/repo/pull/1"),
            create_test_pr(2, "https://github.com/owner/repo/pull/2"),
        ];

        let active = filter_active_prs(prs, &state);
        assert_eq!(active.len(), 2);
    }

    #[test]
    fn test_filter_active_keeps_expired() {
        let mut state = SnoozeState::new();
        let past = Utc::now() - Duration::hours(1);
        state.snooze(
            "https://github.com/owner/repo/pull/1".to_string(),
            Some(past),
        );

        let prs = vec![
            create_test_pr(1, "https://github.com/owner/repo/pull/1"),
            create_test_pr(2, "https://github.com/owner/repo/pull/2"),
        ];

        let active = filter_active_prs(prs, &state);
        assert_eq!(active.len(), 2); // Both should be active (expired snooze counts as active)
    }

    #[test]
    fn test_filter_snoozed_keeps_only_snoozed() {
        let mut state = SnoozeState::new();
        state.snooze("https://github.com/owner/repo/pull/1".to_string(), None);

        let future = Utc::now() + Duration::hours(1);
        state.snooze(
            "https://github.com/owner/repo/pull/3".to_string(),
            Some(future),
        );

        let prs = vec![
            create_test_pr(1, "https://github.com/owner/repo/pull/1"),
            create_test_pr(2, "https://github.com/owner/repo/pull/2"),
            create_test_pr(3, "https://github.com/owner/repo/pull/3"),
        ];

        let snoozed = filter_snoozed_prs(prs, &state);
        assert_eq!(snoozed.len(), 2);
        assert_eq!(snoozed[0].number, 1);
        assert_eq!(snoozed[1].number, 3);
    }
}
