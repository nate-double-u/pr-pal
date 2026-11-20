use super::*;

// `pr_at_index`'s empty-list and out-of-range branches call
// `std::process::exit`, which would terminate the test process, so only
// the happy-path (valid 1-based index) is unit-testable here.
fn test_pr(number: u64) -> pr_pal::github::types::PullRequest {
    pr_pal::github::types::PullRequest {
        number,
        author: "octocat".to_string(),
        url: format!("https://github.com/owner/repo/pull/{}", number),
        additions: 0,
        deletions: 0,
        ..pr_pal::test_support::base_pull_request()
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
