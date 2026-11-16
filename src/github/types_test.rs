use super::*;

fn pr(additions: u64, deletions: u64, filtered_size: Option<u64>) -> PullRequest {
    PullRequest {
        title: "Test PR".to_string(),
        number: 123,
        author: "octocat".to_string(),
        repo: "owner/repo".to_string(),
        url: "https://github.com/owner/repo/pull/123".to_string(),
        created_at: Utc::now() - chrono::Duration::days(5),
        updated_at: Utc::now(),
        additions,
        deletions,
        approvals: 0,
        draft: false,
        labels: Vec::new(),
        user_has_reviewed: false,
        filtered_size,
        signals: ReviewSignals::default(),
    }
}

#[test]
fn age_reflects_time_since_creation() {
    let created_at = Utc::now() - chrono::Duration::days(3);
    let mut p = pr(0, 0, None);
    p.created_at = created_at;

    let age = p.age();

    assert!(age >= chrono::Duration::days(3));
    assert!(age < chrono::Duration::days(4));
}

#[test]
fn size_without_filtered_size_sums_additions_and_deletions() {
    let p = pr(10, 5, None);
    assert_eq!(p.size(), 15);
}

#[test]
fn size_with_filtered_size_prefers_filtered_value() {
    let p = pr(100, 50, Some(7));
    assert_eq!(p.size(), 7);
}

#[test]
fn short_ref_formats_as_owner_repo_hash_number() {
    let mut p = pr(0, 0, None);
    p.repo = "nate-double-u/pr-pal".to_string();
    p.number = 42;
    assert_eq!(p.short_ref(), "nate-double-u/pr-pal#42");
}
