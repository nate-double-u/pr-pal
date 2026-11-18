use super::*;

// `pr_at_index`'s empty-list and out-of-range branches call
// `std::process::exit`, which would terminate the test process, so only
// the happy-path (valid 1-based index) is unit-testable here.
fn test_pr(number: u64) -> pr_pal::github::types::PullRequest {
    pr_pal::github::types::PullRequest {
        title: "Test PR".to_string(),
        number,
        author: "octocat".to_string(),
        repo: "owner/repo".to_string(),
        url: format!("https://github.com/owner/repo/pull/{}", number),
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        additions: 0,
        deletions: 0,
        approvals: 0,
        draft: false,
        labels: Vec::new(),
        user_has_reviewed: false,
        filtered_size: None,
        signals: Default::default(),
    }
}

#[test]
fn pr_at_index_returns_the_pr_at_the_given_1_based_index() {
    let scored_prs: Vec<_> = vec![
        (test_pr(1), pr_pal::scoring::ScoreResult::default()),
        (test_pr(2), pr_pal::scoring::ScoreResult::default()),
        (test_pr(3), pr_pal::scoring::ScoreResult::default()),
    ];

    let pr = pr_at_index(&scored_prs, 2, "pull requests", "open");

    assert_eq!(pr.number, 2);
}
