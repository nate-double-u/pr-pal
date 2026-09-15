use super::types::SnoozeState;
use crate::config::{SuppressConfig, WakeEvent};
use crate::github::types::PullRequest;
use crate::review_state::{review_anchor, ReviewSignals, ReviewState};
use chrono::{DateTime, Duration, Utc};

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

/// Resolved suppression policy (validated config, parsed durations).
#[derive(Debug, Clone)]
pub struct SuppressPolicy {
    pub wake_on: Vec<WakeEvent>,
    /// Safety valve; `None` means suppressed PRs never resurface on time alone
    pub resurface_after: Option<Duration>,
}

/// Build the active suppression policy from config.
/// Returns `Ok(None)` when suppression is absent or disabled.
pub fn suppress_policy(config: Option<&SuppressConfig>) -> Result<Option<SuppressPolicy>, String> {
    match config {
        Some(cfg) if cfg.awaiting_author => Ok(Some(SuppressPolicy {
            wake_on: cfg.wake_on.clone(),
            resurface_after: cfg.resurface_duration()?,
        })),
        _ => Ok(None),
    }
}

/// PRs split by visibility: Active list, suppressed (awaiting author), and
/// manually snoozed.
#[derive(Debug)]
pub struct PartitionedPrs {
    pub active: Vec<PullRequest>,
    pub suppressed: Vec<PullRequest>,
    pub snoozed: Vec<PullRequest>,
}

/// Split PRs into active / suppressed / snoozed.
///
/// Manual snooze always wins. With a suppression policy, reviewed PRs are
/// hidden while awaiting the author; wake events listed in `wake_on` (and the
/// resurface valve) return them to Active.
pub fn partition_prs(
    prs: Vec<PullRequest>,
    snooze_state: &SnoozeState,
    policy: Option<&SuppressPolicy>,
    now: DateTime<Utc>,
) -> PartitionedPrs {
    let mut partitioned = PartitionedPrs {
        active: Vec::new(),
        suppressed: Vec::new(),
        snoozed: Vec::new(),
    };

    for pr in prs {
        if snooze_state.is_snoozed(&pr.url) {
            partitioned.snoozed.push(pr);
            continue;
        }
        let suppressed =
            policy.is_some_and(|policy| is_suppressed_by_policy(&pr.signals, policy, now));
        if suppressed {
            partitioned.suppressed.push(pr);
        } else {
            partitioned.active.push(pr);
        }
    }

    partitioned
}

/// Should this PR be hidden as awaiting-author under the given policy?
///
/// Shared by `partition_prs` and the TUI undo path so suppression decisions
/// always reflect the PR's current signals.
pub fn is_suppressed_by_policy(
    signals: &ReviewSignals,
    policy: &SuppressPolicy,
    now: DateTime<Utc>,
) -> bool {
    effective_review_state(signals, policy, now) == ReviewState::AwaitingAuthor
}

/// The review-cycle state as the active policy sees it.
///
/// The strongest *configured* wake event after the anchor wins (a push must
/// not mask a configured mention); with none, an elapsed valve means
/// `Stalled`; otherwise the PR is `AwaitingAuthor`. This is the single
/// source of truth for both partitioning and the displayed wake tags.
pub fn effective_review_state(
    signals: &ReviewSignals,
    policy: &SuppressPolicy,
    now: DateTime<Utc>,
) -> ReviewState {
    let Some(anchor) = review_anchor(signals) else {
        return ReviewState::NotReviewed;
    };
    let after = |t: Option<DateTime<Utc>>| t.is_some_and(|t| t > anchor);
    let configured = |e: WakeEvent| policy.wake_on.contains(&e);

    if configured(WakeEvent::Push)
        && (after(signals.last_commit_at) || signals.commit_after_my_activity)
    {
        ReviewState::Pushed
    } else if configured(WakeEvent::Mention) && after(signals.mentioned_at) {
        ReviewState::Mentioned
    } else if configured(WakeEvent::ReviewRequest) && after(signals.review_requested_at) {
        ReviewState::ReviewRequested
    } else if valve_elapsed(signals, now, policy.resurface_after) {
        ReviewState::Stalled
    } else {
        ReviewState::AwaitingAuthor
    }
}

/// True when the resurface valve has elapsed since the review anchor.
fn valve_elapsed(
    signals: &ReviewSignals,
    now: DateTime<Utc>,
    resurface_after: Option<Duration>,
) -> bool {
    match (review_anchor(signals), resurface_after) {
        (Some(anchor), Some(valve)) => now - anchor > valve,
        _ => false,
    }
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

    // --- partition_prs / suppression ---

    fn reviewed_pr(number: u64, url: &str, review_days_ago: i64) -> PullRequest {
        let mut pr = create_test_pr(number, url);
        pr.user_has_reviewed = true;
        pr.signals.my_last_review_at = Some(Utc::now() - Duration::days(review_days_ago));
        pr
    }

    fn full_policy() -> SuppressPolicy {
        SuppressPolicy {
            wake_on: vec![
                WakeEvent::Push,
                WakeEvent::Mention,
                WakeEvent::ReviewRequest,
            ],
            resurface_after: Some(Duration::days(21)),
        }
    }

    // LOCKED: regression for since-my-review review workflow (feat/since-my-review).
    // Reviewed PRs awaiting the author must leave the Active list.
    #[test]
    fn partition_awaiting_author_is_suppressed() {
        let prs = vec![
            reviewed_pr(1, "https://github.com/o/r/pull/1", 3),
            create_test_pr(2, "https://github.com/o/r/pull/2"),
        ];
        let result = partition_prs(prs, &SnoozeState::new(), Some(&full_policy()), Utc::now());
        assert_eq!(result.suppressed.len(), 1);
        assert_eq!(result.suppressed[0].number, 1);
        assert_eq!(result.active.len(), 1);
        assert_eq!(result.active[0].number, 2);
        assert!(result.snoozed.is_empty());
    }

    // LOCKED: regression for since-my-review review workflow (feat/since-my-review).
    // No suppress config means no suppression.
    #[test]
    fn partition_without_policy_keeps_awaiting_author_active() {
        let prs = vec![reviewed_pr(1, "https://github.com/o/r/pull/1", 3)];
        let result = partition_prs(prs, &SnoozeState::new(), None, Utc::now());
        assert_eq!(result.active.len(), 1);
        assert!(result.suppressed.is_empty());
    }

    // LOCKED: regression for since-my-review review workflow (feat/since-my-review).
    // Manual snooze always outranks derived suppression.
    #[test]
    fn partition_manual_snooze_wins_over_suppression() {
        let mut state = SnoozeState::new();
        state.snooze("https://github.com/o/r/pull/1".to_string(), None);

        let prs = vec![reviewed_pr(1, "https://github.com/o/r/pull/1", 3)];
        let result = partition_prs(prs, &state, Some(&full_policy()), Utc::now());
        assert_eq!(result.snoozed.len(), 1);
        assert!(result.suppressed.is_empty());
        assert!(result.active.is_empty());
    }

    // LOCKED: regression for since-my-review review workflow (feat/since-my-review).
    // Push/mention/re-request must return a suppressed PR to Active.
    #[test]
    fn partition_wake_events_resurface() {
        let mut pushed = reviewed_pr(1, "https://github.com/o/r/pull/1", 5);
        pushed.signals.last_commit_at = Some(Utc::now() - Duration::days(1));
        let mut mentioned = reviewed_pr(2, "https://github.com/o/r/pull/2", 5);
        mentioned.signals.mentioned_at = Some(Utc::now() - Duration::days(1));
        let mut rerequested = reviewed_pr(3, "https://github.com/o/r/pull/3", 5);
        rerequested.signals.review_requested_at = Some(Utc::now() - Duration::days(1));

        let result = partition_prs(
            vec![pushed, mentioned, rerequested],
            &SnoozeState::new(),
            Some(&full_policy()),
            Utc::now(),
        );
        assert_eq!(result.active.len(), 3);
        assert!(result.suppressed.is_empty());
    }

    // LOCKED: regression for since-my-review review workflow (feat/since-my-review).
    // Events removed from wake_on must not wake a PR.
    #[test]
    fn partition_disabled_wake_event_stays_suppressed() {
        let mut pushed = reviewed_pr(1, "https://github.com/o/r/pull/1", 5);
        pushed.signals.last_commit_at = Some(Utc::now() - Duration::days(1));

        let policy = SuppressPolicy {
            wake_on: vec![WakeEvent::Mention],
            resurface_after: Some(Duration::days(21)),
        };
        let result = partition_prs(vec![pushed], &SnoozeState::new(), Some(&policy), Utc::now());
        assert!(result.active.is_empty());
        assert_eq!(result.suppressed.len(), 1);
    }

    // LOCKED: regression for wake_on subset masking (pr-pal#2 Copilot review).
    // Any configured wake event must wake the PR, even when a higher-priority
    // unconfigured event also occurred (a push must not mask a mention).
    #[test]
    fn partition_configured_wake_fires_despite_higher_priority_event() {
        let mut pr = reviewed_pr(1, "https://github.com/o/r/pull/1", 5);
        pr.signals.last_commit_at = Some(Utc::now() - Duration::days(2));
        pr.signals.mentioned_at = Some(Utc::now() - Duration::days(1));

        let policy = SuppressPolicy {
            wake_on: vec![WakeEvent::Mention],
            resurface_after: Some(Duration::days(21)),
        };
        let result = partition_prs(vec![pr], &SnoozeState::new(), Some(&policy), Utc::now());
        assert_eq!(result.active.len(), 1, "configured mention wake must fire");
        assert!(result.suppressed.is_empty());
    }

    // LOCKED: regression for wake_on subset masking (pr-pal#2 Copilot review).
    // Occurred-but-unconfigured events must still not wake the PR.
    #[test]
    fn partition_mixed_unconfigured_events_stay_suppressed() {
        let mut pr = reviewed_pr(1, "https://github.com/o/r/pull/1", 5);
        pr.signals.last_commit_at = Some(Utc::now() - Duration::days(2));
        pr.signals.mentioned_at = Some(Utc::now() - Duration::days(1));

        let policy = SuppressPolicy {
            wake_on: vec![WakeEvent::ReviewRequest],
            resurface_after: Some(Duration::days(21)),
        };
        let result = partition_prs(vec![pr], &SnoozeState::new(), Some(&policy), Utc::now());
        assert!(result.active.is_empty());
        assert_eq!(result.suppressed.len(), 1);
    }

    // LOCKED: regression for valve masking (pr-pal#2 Copilot review).
    // An occurred-but-unconfigured event must not block the resurface valve:
    // with wake_on [mention] and an old unconfigured push, a PR quiet past
    // resurface_after still resurfaces.
    #[test]
    fn partition_valve_fires_despite_unconfigured_event() {
        let mut pr = reviewed_pr(1, "https://x/1", 30);
        pr.signals.last_commit_at = Some(Utc::now() - Duration::days(25));

        let policy = SuppressPolicy {
            wake_on: vec![WakeEvent::Mention],
            resurface_after: Some(Duration::days(21)),
        };
        let result = partition_prs(vec![pr], &SnoozeState::new(), Some(&policy), Utc::now());
        assert_eq!(result.active.len(), 1, "valve must resurface the PR");
        assert!(result.suppressed.is_empty());
    }

    // LOCKED: regression for policy-aware wake tags (pr-pal#2 Copilot review).
    // The displayed wake reason must come from the same effective policy as
    // partitioning: the strongest *configured* event wins, and unconfigured
    // events fall through to the valve.
    #[test]
    fn effective_state_reports_strongest_configured_event() {
        let now = Utc::now();
        let mut signals = crate::review_state::ReviewSignals {
            my_last_review_at: Some(now - Duration::days(5)),
            ..Default::default()
        };
        signals.last_commit_at = Some(now - Duration::days(2));
        signals.mentioned_at = Some(now - Duration::days(1));

        let policy = SuppressPolicy {
            wake_on: vec![WakeEvent::Mention],
            resurface_after: Some(Duration::days(21)),
        };
        assert_eq!(
            effective_review_state(&signals, &policy, now),
            ReviewState::Mentioned,
            "the configured mention is the effective wake, not the push"
        );
    }

    // LOCKED: regression for policy-aware wake tags (pr-pal#2 Copilot review).
    // With only unconfigured events and the valve elapsed, the effective
    // state is Stalled, matching the partition outcome.
    #[test]
    fn effective_state_stalls_past_valve_despite_unconfigured_event() {
        let now = Utc::now();
        let mut signals = crate::review_state::ReviewSignals {
            my_last_review_at: Some(now - Duration::days(30)),
            ..Default::default()
        };
        signals.last_commit_at = Some(now - Duration::days(25));

        let policy = SuppressPolicy {
            wake_on: vec![WakeEvent::Mention],
            resurface_after: Some(Duration::days(21)),
        };
        assert_eq!(
            effective_review_state(&signals, &policy, now),
            ReviewState::Stalled
        );
    }

    // LOCKED: regression for committer-date vs push-time (pr-pal#2 Copilot review).
    // A late push of old-dated commits must wake a suppressed PR when push
    // is a configured wake event.
    #[test]
    fn effective_state_wakes_on_stream_order_push_flag() {
        let now = Utc::now();
        let mut signals = crate::review_state::ReviewSignals {
            my_last_review_at: Some(now - Duration::days(1)),
            ..Default::default()
        };
        signals.last_commit_at = Some(now - Duration::days(3));
        signals.commit_after_my_activity = true;

        let policy = SuppressPolicy {
            wake_on: vec![WakeEvent::Push],
            resurface_after: Some(Duration::days(21)),
        };
        assert_eq!(
            effective_review_state(&signals, &policy, now),
            ReviewState::Pushed
        );
        assert!(!is_suppressed_by_policy(&signals, &policy, now));
    }

    // LOCKED: regression for since-my-review review workflow (feat/since-my-review).
    // The resurface_after valve must revive quiet PRs.
    #[test]
    fn partition_stalled_resurfaces_via_valve() {
        let prs = vec![reviewed_pr(1, "https://github.com/o/r/pull/1", 22)];
        let result = partition_prs(prs, &SnoozeState::new(), Some(&full_policy()), Utc::now());
        assert_eq!(result.active.len(), 1);
        assert!(result.suppressed.is_empty());
    }

    // LOCKED: regression for since-my-review review workflow (feat/since-my-review).
    // resurface_after: never must keep quiet PRs hidden.
    #[test]
    fn partition_never_valve_keeps_quiet_prs_suppressed() {
        let policy = SuppressPolicy {
            wake_on: vec![WakeEvent::Push],
            resurface_after: None,
        };
        let prs = vec![reviewed_pr(1, "https://github.com/o/r/pull/1", 400)];
        let result = partition_prs(prs, &SnoozeState::new(), Some(&policy), Utc::now());
        assert!(result.active.is_empty());
        assert_eq!(result.suppressed.len(), 1);
    }

    // LOCKED: regression for since-my-review review workflow (feat/since-my-review).
    // Suppression is strictly opt-in.
    #[test]
    fn suppress_policy_disabled_or_absent_is_none() {
        assert!(suppress_policy(None).unwrap().is_none());

        let cfg = SuppressConfig {
            awaiting_author: false,
            wake_on: vec![WakeEvent::Push],
            resurface_after: "21d".to_string(),
        };
        assert!(suppress_policy(Some(&cfg)).unwrap().is_none());
    }

    // LOCKED: regression for since-my-review review workflow (feat/since-my-review).
    // Policy must carry the parsed valve duration.
    #[test]
    fn suppress_policy_resolves_valve() {
        let cfg = SuppressConfig {
            awaiting_author: true,
            wake_on: vec![WakeEvent::Push, WakeEvent::Mention],
            resurface_after: "7d".to_string(),
        };
        let policy = suppress_policy(Some(&cfg)).unwrap().expect("enabled");
        assert_eq!(policy.resurface_after, Some(Duration::days(7)));
        assert_eq!(policy.wake_on, vec![WakeEvent::Push, WakeEvent::Mention]);
    }

    // LOCKED: regression for since-my-review review workflow (feat/since-my-review).
    // Bad durations must fail, not silently disable the valve.
    #[test]
    fn suppress_policy_propagates_invalid_duration() {
        let cfg = SuppressConfig {
            awaiting_author: true,
            wake_on: vec![WakeEvent::Push],
            resurface_after: "eleventy".to_string(),
        };
        let err = suppress_policy(Some(&cfg)).unwrap_err();
        assert!(err.contains("suppress.resurface_after"));
    }
}
