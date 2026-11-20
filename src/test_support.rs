//! Shared test fixtures for building `PullRequest` values in unit tests.
//!
//! Several test modules across the crate each hand-rolled their own
//! `PullRequest` literal with the same boilerplate fields. [`base_pull_request`]
//! centralizes those defaults so call sites only need to list the fields a
//! given test actually cares about, via struct-update syntax:
//!
//! ```ignore
//! PullRequest {
//!     number: 42,
//!     approvals: 2,
//!     ..crate::test_support::base_pull_request()
//! }
//! ```
//!
//! This module is not gated behind `#[cfg(test)]` because `src/main_test.rs`
//! depends on it as an external user of the `pr_pal` library crate, where a
//! `cfg(test)`-only item compiled for the lib's own test build would not be
//! visible.

use crate::github::types::PullRequest;
use crate::review_state::ReviewSignals;
use chrono::Utc;

/// A `PullRequest` with representative, non-edge-case defaults for every
/// field. Override only the fields a specific test needs via struct-update
/// syntax (`..base_pull_request()`).
pub fn base_pull_request() -> PullRequest {
    PullRequest {
        title: "Test PR".to_string(),
        number: 1,
        author: "author".to_string(),
        repo: "owner/repo".to_string(),
        url: "https://github.com/owner/repo/pull/1".to_string(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
        additions: 10,
        deletions: 5,
        approvals: 0,
        draft: false,
        labels: vec![],
        user_has_reviewed: false,
        filtered_size: None,
        signals: ReviewSignals::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_pull_request_has_stable_defaults() {
        let pr = base_pull_request();
        assert_eq!(pr.title, "Test PR");
        assert_eq!(pr.number, 1);
        assert_eq!(pr.repo, "owner/repo");
        assert_eq!(pr.additions, 10);
        assert_eq!(pr.deletions, 5);
        assert_eq!(pr.approvals, 0);
        assert!(!pr.draft);
        assert!(pr.labels.is_empty());
        assert!(!pr.user_has_reviewed);
        assert_eq!(pr.filtered_size, None);
    }

    #[test]
    fn struct_update_syntax_overrides_only_named_fields() {
        let pr = PullRequest {
            number: 42,
            approvals: 3,
            ..base_pull_request()
        };
        assert_eq!(pr.number, 42);
        assert_eq!(pr.approvals, 3);
        // Untouched fields keep the shared defaults.
        assert_eq!(pr.title, "Test PR");
        assert_eq!(pr.repo, "owner/repo");
    }
}
