//! Derived review-cycle state for PRs the user has already reviewed.
//!
//! This is computed fresh from PR data on every refresh (no persistence).
//! The predicate answers: "since my last activity on this PR, what happened?"
//! Consumers apply policy on top: the suppress filter hides `AwaitingAuthor`
//! PRs from the Active list, and the scoring engine boosts wake states.

use chrono::{DateTime, Duration, Utc};

/// Timestamps extracted during enrichment that drive the review-cycle state.
///
/// `my_last_review_at` is the anchor requirement: without a review by the
/// user, there is no review cycle. `my_last_comment_at` extends the anchor,
/// so nudging an author re-arms the cycle without a formal re-review.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ReviewSignals {
    /// Latest review submitted by the authenticated user
    pub my_last_review_at: Option<DateTime<Utc>>,
    /// Latest issue comment by the authenticated user
    pub my_last_comment_at: Option<DateTime<Utc>>,
    /// Latest commit on the PR (committer date)
    pub last_commit_at: Option<DateTime<Utc>>,
    /// Latest @-mention of the authenticated user
    pub mentioned_at: Option<DateTime<Utc>>,
    /// Latest review request targeting the authenticated user
    pub review_requested_at: Option<DateTime<Utc>>,
    /// A commit event appeared after the user's last review/comment in the
    /// timeline stream, even if its committer date is older (old local
    /// commits pushed late keep their original dates)
    pub commit_after_my_activity: bool,
}

/// What happened since the user's last activity on a PR they reviewed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewState {
    /// User has never reviewed this PR; the review cycle does not apply
    NotReviewed,
    /// User reviewed; nothing notable since. Ball is in the author's court.
    AwaitingAuthor,
    /// New commits landed after the user's last activity
    Pushed,
    /// User was @-mentioned after their last activity
    Mentioned,
    /// User's review was re-requested after their last activity
    ReviewRequested,
    /// Nothing happened for longer than the resurface valve
    Stalled,
}

/// The review-cycle anchor: the user's most recent activity (review or
/// comment), gated on at least one review existing.
pub fn review_anchor(signals: &ReviewSignals) -> Option<DateTime<Utc>> {
    let last_review = signals.my_last_review_at?;
    Some(match signals.my_last_comment_at {
        Some(comment) if comment > last_review => comment,
        _ => last_review,
    })
}

/// Compute the review-cycle state from signals.
///
/// The anchor is the user's most recent activity (review or comment), gated
/// on at least one review existing. Events strictly after the anchor wake the
/// PR, strongest signal first: Pushed > Mentioned > ReviewRequested. With no
/// wake event, `resurface_after` (the safety valve) turns a long-quiet PR
/// `Stalled`; `None` disables the valve.
pub fn review_state(
    signals: &ReviewSignals,
    now: DateTime<Utc>,
    resurface_after: Option<Duration>,
) -> ReviewState {
    let Some(anchor) = review_anchor(signals) else {
        return ReviewState::NotReviewed;
    };

    let after_anchor = |t: Option<DateTime<Utc>>| t.is_some_and(|t| t > anchor);

    if after_anchor(signals.last_commit_at) || signals.commit_after_my_activity {
        ReviewState::Pushed
    } else if after_anchor(signals.mentioned_at) {
        ReviewState::Mentioned
    } else if after_anchor(signals.review_requested_at) {
        ReviewState::ReviewRequested
    } else if resurface_after.is_some_and(|valve| now - anchor > valve) {
        ReviewState::Stalled
    } else {
        ReviewState::AwaitingAuthor
    }
}

// LOCKED: regression for since-my-review review workflow (feat/since-my-review).
// All tests in this module are locked. Review-cycle state predicate: the feature's core specification.
#[cfg(test)]
mod tests {
    use super::*;

    fn days_ago(now: DateTime<Utc>, days: i64) -> DateTime<Utc> {
        now - Duration::days(days)
    }

    fn valve() -> Option<Duration> {
        Some(Duration::days(21))
    }

    #[test]
    fn never_reviewed_is_not_reviewed_even_with_other_signals() {
        let now = Utc::now();
        let signals = ReviewSignals {
            my_last_review_at: None,
            my_last_comment_at: Some(days_ago(now, 2)),
            last_commit_at: Some(days_ago(now, 1)),
            mentioned_at: Some(days_ago(now, 1)),
            review_requested_at: Some(days_ago(now, 1)),
            commit_after_my_activity: true,
        };
        assert_eq!(
            review_state(&signals, now, valve()),
            ReviewState::NotReviewed
        );
    }

    #[test]
    fn reviewed_with_no_later_activity_is_awaiting_author() {
        let now = Utc::now();
        let signals = ReviewSignals {
            my_last_review_at: Some(days_ago(now, 3)),
            ..Default::default()
        };
        assert_eq!(
            review_state(&signals, now, valve()),
            ReviewState::AwaitingAuthor
        );
    }

    #[test]
    fn commit_after_review_is_pushed() {
        let now = Utc::now();
        let signals = ReviewSignals {
            my_last_review_at: Some(days_ago(now, 3)),
            last_commit_at: Some(days_ago(now, 1)),
            ..Default::default()
        };
        assert_eq!(review_state(&signals, now, valve()), ReviewState::Pushed);
    }

    #[test]
    fn commit_before_review_is_awaiting_author() {
        let now = Utc::now();
        let signals = ReviewSignals {
            my_last_review_at: Some(days_ago(now, 1)),
            last_commit_at: Some(days_ago(now, 3)),
            ..Default::default()
        };
        assert_eq!(
            review_state(&signals, now, valve()),
            ReviewState::AwaitingAuthor
        );
    }

    // LOCKED: regression for committer-date vs push-time (pr-pal#2 Copilot review).
    // A commit with an old committer date pushed after my review must wake the PR.
    #[test]
    fn late_push_with_old_commit_date_is_pushed() {
        let now = Utc::now();
        let signals = ReviewSignals {
            my_last_review_at: Some(days_ago(now, 1)),
            last_commit_at: Some(days_ago(now, 3)),
            commit_after_my_activity: true,
            ..Default::default()
        };
        assert_eq!(review_state(&signals, now, valve()), ReviewState::Pushed);
    }

    #[test]
    fn commit_exactly_at_anchor_is_not_after() {
        let now = Utc::now();
        let t = days_ago(now, 2);
        let signals = ReviewSignals {
            my_last_review_at: Some(t),
            last_commit_at: Some(t),
            ..Default::default()
        };
        assert_eq!(
            review_state(&signals, now, valve()),
            ReviewState::AwaitingAuthor
        );
    }

    #[test]
    fn mention_after_review_is_mentioned() {
        let now = Utc::now();
        let signals = ReviewSignals {
            my_last_review_at: Some(days_ago(now, 3)),
            mentioned_at: Some(days_ago(now, 1)),
            ..Default::default()
        };
        assert_eq!(review_state(&signals, now, valve()), ReviewState::Mentioned);
    }

    #[test]
    fn review_request_after_review_is_review_requested() {
        let now = Utc::now();
        let signals = ReviewSignals {
            my_last_review_at: Some(days_ago(now, 3)),
            review_requested_at: Some(days_ago(now, 1)),
            ..Default::default()
        };
        assert_eq!(
            review_state(&signals, now, valve()),
            ReviewState::ReviewRequested
        );
    }

    #[test]
    fn pushed_wins_over_mentioned_and_review_requested() {
        let now = Utc::now();
        let signals = ReviewSignals {
            my_last_review_at: Some(days_ago(now, 5)),
            last_commit_at: Some(days_ago(now, 1)),
            mentioned_at: Some(days_ago(now, 1)),
            review_requested_at: Some(days_ago(now, 1)),
            ..Default::default()
        };
        assert_eq!(review_state(&signals, now, valve()), ReviewState::Pushed);
    }

    #[test]
    fn mentioned_wins_over_review_requested() {
        let now = Utc::now();
        let signals = ReviewSignals {
            my_last_review_at: Some(days_ago(now, 5)),
            mentioned_at: Some(days_ago(now, 1)),
            review_requested_at: Some(days_ago(now, 1)),
            ..Default::default()
        };
        assert_eq!(review_state(&signals, now, valve()), ReviewState::Mentioned);
    }

    #[test]
    fn quiet_past_the_valve_is_stalled() {
        let now = Utc::now();
        let signals = ReviewSignals {
            my_last_review_at: Some(days_ago(now, 22)),
            ..Default::default()
        };
        assert_eq!(review_state(&signals, now, valve()), ReviewState::Stalled);
    }

    #[test]
    fn quiet_within_the_valve_is_awaiting_author() {
        let now = Utc::now();
        let signals = ReviewSignals {
            my_last_review_at: Some(days_ago(now, 20)),
            ..Default::default()
        };
        assert_eq!(
            review_state(&signals, now, valve()),
            ReviewState::AwaitingAuthor
        );
    }

    #[test]
    fn wake_event_wins_over_stalled() {
        let now = Utc::now();
        let signals = ReviewSignals {
            my_last_review_at: Some(days_ago(now, 40)),
            mentioned_at: Some(days_ago(now, 30)),
            ..Default::default()
        };
        assert_eq!(review_state(&signals, now, valve()), ReviewState::Mentioned);
    }

    #[test]
    fn no_valve_never_stalls() {
        let now = Utc::now();
        let signals = ReviewSignals {
            my_last_review_at: Some(days_ago(now, 400)),
            ..Default::default()
        };
        assert_eq!(
            review_state(&signals, now, None),
            ReviewState::AwaitingAuthor
        );
    }

    #[test]
    fn my_comment_extends_the_anchor() {
        let now = Utc::now();
        // Review 30d ago, author pushed 10d ago, I replied 5d ago:
        // ball is back in the author's court and the valve clock restarts.
        let signals = ReviewSignals {
            my_last_review_at: Some(days_ago(now, 30)),
            last_commit_at: Some(days_ago(now, 10)),
            my_last_comment_at: Some(days_ago(now, 5)),
            ..Default::default()
        };
        assert_eq!(
            review_state(&signals, now, valve()),
            ReviewState::AwaitingAuthor
        );
    }

    #[test]
    fn comment_alone_without_review_does_not_start_a_cycle() {
        let now = Utc::now();
        let signals = ReviewSignals {
            my_last_comment_at: Some(days_ago(now, 2)),
            ..Default::default()
        };
        assert_eq!(
            review_state(&signals, now, valve()),
            ReviewState::NotReviewed
        );
    }
}
