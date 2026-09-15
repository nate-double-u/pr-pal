use crate::buffered_eprintln;
use anyhow::{anyhow, Context, Result};
use futures::stream::{FuturesUnordered, StreamExt};
use octocrab::Octocrab;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::github::types::PullRequest;

/// Search GitHub for pull requests matching the given query.
/// Auth errors (401 / Bad credentials) fail immediately as a typed AuthError.
/// Rate limit and permission errors also fail immediately.
/// Transient/network errors are retried up to 3 times with exponential backoff.
pub async fn search_prs(client: &Octocrab, query: &str) -> Result<Vec<PullRequest>> {
    // Ensure the query only returns PRs, not issues
    let query = if query.contains("is:pr") {
        query.to_string()
    } else {
        format!("{} is:pr", query)
    };

    let max_retries = 3;
    let mut attempt = 0;

    loop {
        attempt += 1;
        match client
            .search()
            .issues_and_pull_requests(&query)
            .per_page(100u8)
            .send()
            .await
        {
            Ok(first_page) => {
                // Follow pagination so multi-page result sets are fully
                // collected. Without this, GitHub's default page size caps
                // each query at its first page (see the pagination regression
                // test below).
                let items = match client.all_pages(first_page).await {
                    Ok(items) => items,
                    Err(e) => return Err(anyhow!("Failed to paginate search results: {}", e)),
                };
                let prs: Vec<PullRequest> = items
                    .into_iter()
                    .filter(|issue| issue.pull_request.is_some()) // Only PRs, not issues
                    .map(|issue| {
                        // Extract owner/repo from html_url
                        // Format: "https://github.com/owner/repo/pull/123"
                        let path = issue.html_url.path();
                        let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
                        let repo = if parts.len() >= 2 {
                            format!("{}/{}", parts[0], parts[1])
                        } else {
                            "unknown/unknown".to_string()
                        };

                        PullRequest {
                            title: issue.title,
                            number: issue.number,
                            author: issue.user.login.clone(),
                            repo,
                            url: issue.html_url.to_string(),
                            created_at: issue.created_at,
                            updated_at: issue.updated_at,
                            additions: 0, // Search API doesn't include these
                            deletions: 0, // Will be populated by enrichment
                            approvals: 0, // Requires separate API call
                            draft: false, // Populated during enrichment from Pulls API
                            labels: issue.labels.iter().map(|l| l.name.clone()).collect(),
                            user_has_reviewed: false, // Will be populated by enrichment
                            filtered_size: None, // Will be set by enrich_pr if exclude patterns configured
                            signals: Default::default(), // Populated by enrichment
                        }
                    })
                    .collect();
                return Ok(prs);
            }
            Err(e) => {
                let error_str = format!("{:?}", e);

                // Auth errors: fail immediately with typed AuthError (no retry)
                if error_str.contains("401") || error_str.contains("Bad credentials") {
                    return Err(crate::fetch::AuthError {
                        message:
                            "Authentication failed. Your GitHub token may be invalid or expired."
                                .to_string(),
                    }
                    .into());
                }

                // Rate limit: fail immediately (caller handles differently)
                if error_str.contains("rate limit") || error_str.contains("403") {
                    return Err(anyhow!(
                        "GitHub API rate limit exceeded. Wait a few minutes and try again."
                    ));
                }

                // Permission errors: fail immediately
                if error_str.contains("do not have permission")
                    || error_str.contains("resources do not exist")
                {
                    return Err(anyhow!("Repository not found or no access. Check repo name and token permissions (needs 'repo' scope for private repos)."));
                }

                // Transient errors: retry with backoff
                if attempt >= max_retries {
                    return Err(anyhow!(
                        "GitHub API error after {} attempts: {}",
                        max_retries,
                        e
                    ));
                }

                let delay = std::time::Duration::from_millis(100 * (1 << (attempt - 1))); // 100ms, 200ms, 400ms
                tokio::time::sleep(delay).await;
            }
        }
    }
}

/// Fetch PR details (additions, deletions) from the GitHub API
async fn fetch_pr_details(
    client: &Octocrab,
    owner: &str,
    repo: &str,
    number: u64,
) -> Result<(u64, u64, bool)> {
    let pr = client
        .pulls(owner, repo)
        .get(number)
        .await
        .context("Failed to fetch PR details")?;

    let additions = pr.additions.unwrap_or(0);
    let deletions = pr.deletions.unwrap_or(0);
    let draft = pr.draft.unwrap_or(false);

    Ok((additions, deletions, draft))
}

/// Fetch PR review count (approved reviews), whether the authenticated user
/// has reviewed, and the user's latest review timestamp
async fn fetch_pr_reviews(
    client: &Octocrab,
    owner: &str,
    repo: &str,
    number: u64,
    auth_username: Option<&str>,
) -> Result<(u32, bool, Option<chrono::DateTime<chrono::Utc>>)> {
    let first_page = client
        .pulls(owner, repo)
        .list_reviews(number)
        .per_page(100)
        .send()
        .await
        .context("Failed to fetch PR reviews")?;
    let reviews = client
        .all_pages(first_page)
        .await
        .context("Failed to fetch PR review pages")?;

    let approved_count = reviews
        .iter()
        .filter(|review| {
            matches!(
                review.state,
                Some(octocrab::models::pulls::ReviewState::Approved)
            )
        })
        .count() as u32;

    // The user's reviews: any state counts, latest submitted_at is the anchor
    let my_last_review_at = auth_username.and_then(|username| {
        reviews
            .iter()
            .filter(|r| {
                r.user
                    .as_ref()
                    .is_some_and(|u| u.login.eq_ignore_ascii_case(username))
            })
            .filter_map(|r| r.submitted_at)
            .max()
    });
    let user_has_reviewed = auth_username.is_some_and(|username| {
        reviews.iter().any(|r| {
            r.user
                .as_ref()
                .is_some_and(|u| u.login.eq_ignore_ascii_case(username))
        })
    });

    Ok((approved_count, user_has_reviewed, my_last_review_at))
}

/// Fetch all issue timeline events for a PR (paginated).
async fn fetch_pr_timeline(
    client: &Octocrab,
    owner: &str,
    repo: &str,
    number: u64,
) -> Result<Vec<serde_json::Value>> {
    const PER_PAGE: usize = 100;
    let mut events = Vec::new();
    let mut page: u32 = 1;
    loop {
        let route = format!(
            "/repos/{}/{}/issues/{}/timeline?per_page={}&page={}",
            owner, repo, number, PER_PAGE, page
        );
        let batch: Vec<serde_json::Value> = client
            .get(route, None::<&()>)
            .await
            .context("Failed to fetch PR timeline")?;
        let batch_len = batch.len();
        events.extend(batch);
        if batch_len < PER_PAGE {
            break;
        }
        page += 1;
    }
    Ok(events)
}

/// Fetch per-file diff data for a PR with pagination.
/// Returns a list of (filename, additions, deletions) tuples.
async fn fetch_pr_file_list(
    client: &Octocrab,
    owner: &str,
    repo: &str,
    number: u64,
) -> Result<Vec<(String, u64, u64)>> {
    let page = client
        .pulls(owner, repo)
        .list_files(number)
        .await
        .context("Failed to fetch PR file list")?;

    let all_files = client
        .all_pages(page)
        .await
        .context("Failed to paginate PR file list")?;

    Ok(all_files
        .into_iter()
        .map(|f| (f.filename, f.additions, f.deletions))
        .collect())
}

/// Filter files by basename glob matching and compute total size of non-excluded files.
fn apply_size_exclusions(files: &[(String, u64, u64)], exclude_patterns: &[String]) -> Result<u64> {
    let compiled: Vec<glob::Pattern> = exclude_patterns
        .iter()
        .map(|p| glob::Pattern::new(p).context(format!("Invalid glob pattern: {}", p)))
        .collect::<Result<Vec<_>>>()?;

    let total = files
        .iter()
        .filter(|(filename, _, _)| {
            let basename = std::path::Path::new(filename)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(filename);
            !compiled.iter().any(|pat| pat.matches(basename))
        })
        .map(|(_, additions, deletions)| additions + deletions)
        .sum();

    Ok(total)
}

/// Partial review-cycle signals extracted from issue timeline events.
#[derive(Debug, Default, PartialEq)]
struct TimelineSignals {
    last_commit_at: Option<chrono::DateTime<chrono::Utc>>,
    mentioned_at: Option<chrono::DateTime<chrono::Utc>>,
    review_requested_at: Option<chrono::DateTime<chrono::Utc>>,
    my_last_comment_at: Option<chrono::DateTime<chrono::Utc>>,
    commit_after_my_activity: bool,
}

/// Extract review-cycle signals from raw timeline events.
///
/// Recognized events (all others ignored):
/// - `committed`: commit committer date -> last_commit_at
/// - `head_ref_force_pushed`: event created_at -> last_commit_at (rebases can
///   carry old committer dates, so the force-push event marks the update)
/// - `mentioned` where actor is the user -> mentioned_at
/// - `review_requested` where requested_reviewer is the user -> review_requested_at
/// - `commented` where actor is the user -> my_last_comment_at
/// - `reviewed` where user is the user -> stream-order activity marker
///
/// Commit committer dates are commit metadata: old local commits pushed after
/// a review keep their old dates. The timeline stream is ordered by when
/// events reached GitHub, so a commit event *appearing after* the user's last
/// `reviewed`/`commented` event sets `commit_after_my_activity` regardless of
/// its date.
fn parse_timeline_events(
    events: &[serde_json::Value],
    auth_username: Option<&str>,
) -> TimelineSignals {
    use chrono::{DateTime, Utc};

    fn parse_date(value: &serde_json::Value) -> Option<DateTime<Utc>> {
        value.as_str().and_then(|s| s.parse().ok())
    }

    fn max_ts(current: &mut Option<DateTime<Utc>>, candidate: Option<DateTime<Utc>>) {
        if let Some(candidate) = candidate {
            if current.is_none_or(|existing| candidate > existing) {
                *current = Some(candidate);
            }
        }
    }

    fn login_matches(value: &serde_json::Value, auth_username: Option<&str>) -> bool {
        match (value["login"].as_str(), auth_username) {
            (Some(login), Some(user)) => login.eq_ignore_ascii_case(user),
            _ => false,
        }
    }

    let mut signals = TimelineSignals::default();
    let mut last_commit_idx: Option<usize> = None;
    let mut last_my_activity_idx: Option<usize> = None;
    for (idx, event) in events.iter().enumerate() {
        match event["event"].as_str() {
            Some("committed") => {
                max_ts(
                    &mut signals.last_commit_at,
                    parse_date(&event["committer"]["date"]),
                );
                last_commit_idx = Some(idx);
            }
            Some("head_ref_force_pushed") => {
                max_ts(
                    &mut signals.last_commit_at,
                    parse_date(&event["created_at"]),
                );
                last_commit_idx = Some(idx);
            }
            Some("mentioned") if login_matches(&event["actor"], auth_username) => {
                max_ts(&mut signals.mentioned_at, parse_date(&event["created_at"]));
            }
            Some("review_requested")
                if login_matches(&event["requested_reviewer"], auth_username) =>
            {
                max_ts(
                    &mut signals.review_requested_at,
                    parse_date(&event["created_at"]),
                );
            }
            Some("commented") if login_matches(&event["actor"], auth_username) => {
                max_ts(
                    &mut signals.my_last_comment_at,
                    parse_date(&event["created_at"]),
                );
                last_my_activity_idx = Some(idx);
            }
            Some("reviewed") if login_matches(&event["user"], auth_username) => {
                last_my_activity_idx = Some(idx);
            }
            _ => {}
        }
    }
    signals.commit_after_my_activity = matches!(
        (last_commit_idx, last_my_activity_idx),
        (Some(commit), Some(activity)) if commit > activity
    );
    signals
}

/// Enrich a PR with detailed information (size and approvals)
async fn enrich_pr(
    client: &Octocrab,
    pr: &mut PullRequest,
    auth_username: Option<&str>,
    exclude_patterns: &Option<Vec<String>>,
    need_signals: bool,
) -> Result<()> {
    // Parse owner/repo from pr.repo field
    let parts: Vec<&str> = pr.repo.split('/').collect();
    if parts.len() != 2 {
        return Err(anyhow!("Invalid repo format: {}", pr.repo));
    }
    let owner = parts[0];
    let repo_name = parts[1];

    // Fetch details and reviews in parallel
    let details_fut = fetch_pr_details(client, owner, repo_name, pr.number);
    let reviews_fut = fetch_pr_reviews(client, owner, repo_name, pr.number, auth_username);

    match tokio::try_join!(details_fut, reviews_fut) {
        Ok(((additions, deletions, draft), (approvals, user_has_reviewed, my_last_review_at))) => {
            pr.additions = additions;
            pr.deletions = deletions;
            pr.draft = draft;
            pr.approvals = approvals;
            pr.user_has_reviewed = user_has_reviewed;
            pr.signals.my_last_review_at = my_last_review_at;

            // Review-cycle signals need a timeline walk; only pay for it when
            // the feature is configured and the user has actually reviewed.
            if need_signals && user_has_reviewed {
                match fetch_pr_timeline(client, owner, repo_name, pr.number).await {
                    Ok(events) => {
                        let timeline = parse_timeline_events(&events, auth_username);
                        pr.signals.my_last_comment_at = timeline.my_last_comment_at;
                        pr.signals.last_commit_at = timeline.last_commit_at;
                        pr.signals.mentioned_at = timeline.mentioned_at;
                        pr.signals.review_requested_at = timeline.review_requested_at;
                        pr.signals.commit_after_my_activity = timeline.commit_after_my_activity;
                    }
                    Err(e) => {
                        // Fail open: without timeline data the review anchor
                        // would classify this PR as awaiting-author and hide
                        // it. Clear the anchor so it stays active;
                        // user_has_reviewed is kept for legacy scoring.
                        pr.signals.my_last_review_at = None;
                        if is_rate_limit_error(&e) {
                            // Propagate so the caller trips the shared stop
                            // flag instead of hammering a spent rate limit.
                            return Err(e);
                        }
                        buffered_eprintln!(
                            "Warning: Failed to fetch timeline for PR {}: {}",
                            pr.number,
                            e
                        );
                    }
                }
            }

            // Conditionally fetch per-file data and apply size exclusions
            if let Some(ref patterns) = exclude_patterns {
                if !patterns.is_empty() {
                    match fetch_pr_file_list(client, owner, repo_name, pr.number).await {
                        Ok(files) => {
                            match apply_size_exclusions(&files, patterns) {
                                Ok(filtered) => pr.filtered_size = Some(filtered),
                                Err(e) => {
                                    buffered_eprintln!(
                                        "Warning: Failed to apply size exclusions for PR {}: {}",
                                        pr.number,
                                        e
                                    );
                                    // Leave filtered_size as None — fallback to aggregate size
                                }
                            }
                        }
                        Err(e) => {
                            buffered_eprintln!(
                                "Warning: Failed to fetch file list for PR {}: {}",
                                pr.number,
                                e
                            );
                            // Leave filtered_size as None — fallback to aggregate size
                        }
                    }
                }
            }

            Ok(())
        }
        Err(e) => {
            if is_rate_limit_error(&e) {
                // Propagate so the caller trips the shared stop flag.
                return Err(e);
            }
            // If enrichment fails, log but don't fail the whole operation
            buffered_eprintln!("Warning: Failed to enrich PR {}: {}", pr.number, e);
            Ok(())
        }
    }
}

/// True when an error is a GitHub rate limit.
///
/// Prefers the structured octocrab error (recoverable through anyhow
/// context): primary and secondary limits both carry an explicit "rate
/// limit" message, and secondary limits may use HTTP 429. A bare 403 is
/// permissions/forbidden, not a rate limit, and "403" appearing in error
/// text (URLs, status lines) proves nothing, so status text is never
/// matched.
fn is_rate_limit_error(err: &anyhow::Error) -> bool {
    if let Some(octocrab::Error::GitHub { source, .. }) = err.downcast_ref::<octocrab::Error>() {
        return source.status_code.as_u16() == 429
            || source.message.to_lowercase().contains("rate limit");
    }
    // Fallback for non-GitHub-typed errors: the explicit phrase only, over
    // the full chain so context wrappers don't mask it.
    format!("{err:#}").to_lowercase().contains("rate limit")
}

/// Helper function for concurrent PR enrichment
async fn enrich_pr_with_rate_limit_check(
    client: Octocrab,
    mut pr: PullRequest,
    rate_limited: Arc<AtomicBool>,
    auth_username: Option<String>,
    exclude_patterns: Option<Vec<String>>,
    need_signals: bool,
) -> PullRequest {
    if rate_limited.load(Ordering::Relaxed) {
        return pr; // Skip enrichment if rate limited
    }

    match enrich_pr(
        &client,
        &mut pr,
        auth_username.as_deref(),
        &exclude_patterns,
        need_signals,
    )
    .await
    {
        Ok(_) => {}
        Err(e) => {
            if is_rate_limit_error(&e) {
                buffered_eprintln!(
                    "Warning: Rate limit hit during enrichment. Returning partial results."
                );
                rate_limited.store(true, Ordering::Relaxed);
            } else {
                buffered_eprintln!("Warning: Failed to enrich PR {}: {}", pr.number, e);
            }
        }
    }
    pr
}

/// Search and enrich PRs with full details.
///
/// `need_signals` enables the per-PR timeline walk that powers the
/// since-my-review scoring factor and awaiting-author suppression; it is
/// only performed for PRs the user has reviewed.
pub async fn search_and_enrich_prs(
    client: &Octocrab,
    query: &str,
    auth_username: Option<&str>,
    exclude_patterns: Option<Vec<String>>,
    need_signals: bool,
) -> Result<Vec<PullRequest>> {
    let prs = search_prs(client, query).await?;

    // Enrich PRs with bounded concurrency
    const MAX_CONCURRENT_ENRICHMENTS: usize = 10;

    // Rate limit flag shared across concurrent tasks
    let rate_limited = Arc::new(AtomicBool::new(false));

    let mut futures = FuturesUnordered::new();
    let mut prs_iter = prs.into_iter();
    let mut enriched_prs = Vec::new();

    // Fill initial batch
    for _ in 0..MAX_CONCURRENT_ENRICHMENTS {
        if let Some(pr) = prs_iter.next() {
            futures.push(enrich_pr_with_rate_limit_check(
                client.clone(),
                pr,
                rate_limited.clone(),
                auth_username.map(|s| s.to_string()),
                exclude_patterns.clone(),
                need_signals,
            ));
        }
    }

    // Process results and feed new tasks
    while let Some(pr) = futures.next().await {
        enriched_prs.push(pr);

        // Add next PR if not rate limited
        if !rate_limited.load(Ordering::Relaxed) {
            if let Some(next_pr) = prs_iter.next() {
                futures.push(enrich_pr_with_rate_limit_check(
                    client.clone(),
                    next_pr,
                    rate_limited.clone(),
                    auth_username.map(|s| s.to_string()),
                    exclude_patterns.clone(),
                    need_signals,
                ));
            }
        }
    }

    // Add any remaining unenriched PRs (if rate limited, remaining weren't submitted)
    enriched_prs.extend(prs_iter);

    Ok(enriched_prs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{method, path, query_param, query_param_is_missing};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn author_json(login: &str) -> serde_json::Value {
        let base = format!("https://api.github.com/users/{login}");
        json!({
            "login": login,
            "id": 1,
            "node_id": "U1",
            "avatar_url": "https://avatars.githubusercontent.com/u/1",
            "gravatar_id": "",
            "url": base,
            "html_url": format!("https://github.com/{login}"),
            "followers_url": format!("{base}/followers"),
            "following_url": format!("{base}/following{{/other_user}}"),
            "gists_url": format!("{base}/gists{{/gist_id}}"),
            "starred_url": format!("{base}/starred{{/owner}}{{/repo}}"),
            "subscriptions_url": format!("{base}/subscriptions"),
            "organizations_url": format!("{base}/orgs"),
            "repos_url": format!("{base}/repos"),
            "events_url": format!("{base}/events{{/privacy}}"),
            "received_events_url": format!("{base}/received_events"),
            "type": "User",
            "site_admin": false,
            "name": null,
            "patch_url": null,
            "email": null
        })
    }

    /// A minimal but fully deserializable octocrab `Issue` JSON representing a PR.
    fn pr_issue_json(number: u64, owner: &str, repo: &str) -> serde_json::Value {
        let html_url = format!("https://github.com/{owner}/{repo}/pull/{number}");
        let api = format!("https://api.github.com/repos/{owner}/{repo}/issues/{number}");
        json!({
            "id": number,
            "node_id": format!("I{number}"),
            "url": api,
            "repository_url": format!("https://api.github.com/repos/{owner}/{repo}"),
            "labels_url": format!("{api}/labels{{/name}}"),
            "comments_url": format!("{api}/comments"),
            "events_url": format!("{api}/events"),
            "html_url": html_url,
            "number": number,
            "state": "open",
            "state_reason": null,
            "title": format!("PR {number}"),
            "body": null,
            "user": author_json(owner),
            "labels": [],
            "assignee": null,
            "assignees": [],
            "author_association": null,
            "milestone": null,
            "locked": false,
            "active_lock_reason": null,
            "comments": 0,
            "pull_request": {
                "url": api,
                "html_url": html_url,
                "diff_url": format!("{html_url}.diff"),
                "patch_url": format!("{html_url}.patch")
            },
            "closed_at": null,
            "closed_by": null,
            "created_at": "2026-01-01T00:00:00Z",
            "updated_at": "2026-01-01T00:00:00Z"
        })
    }

    fn search_body(items: Vec<serde_json::Value>) -> serde_json::Value {
        json!({
            "total_count": items.len(),
            "incomplete_results": false,
            "items": items
        })
    }

    // LOCKED: regression for search pagination (30-result cap; upstream toniperic/pr-bro).
    // search_prs must follow the Link header and return results beyond the first page.
    #[tokio::test]
    async fn search_prs_returns_results_from_all_pages() {
        let server = MockServer::start().await;

        // Page 2: one more PR, no further pages.
        Mock::given(method("GET"))
            .and(path("/search/issues"))
            .and(query_param("page", "2"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(search_body(vec![pr_issue_json(3, "o", "r")])),
            )
            .mount(&server)
            .await;

        // Page 1: two PRs plus a Link header advertising page 2.
        let next_link = format!(
            "<{}/search/issues?q=x&per_page=100&page=2>; rel=\"next\"",
            server.uri()
        );
        Mock::given(method("GET"))
            .and(path("/search/issues"))
            .and(query_param_is_missing("page"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("link", next_link.as_str())
                    .set_body_json(search_body(vec![
                        pr_issue_json(1, "o", "r"),
                        pr_issue_json(2, "o", "r"),
                    ])),
            )
            .mount(&server)
            .await;

        let client = Octocrab::builder()
            .base_uri(server.uri())
            .unwrap()
            .build()
            .unwrap();

        let prs = search_prs(&client, "review-requested:@me is:pr")
            .await
            .expect("search_prs should succeed");

        // First page alone has 2; only true pagination yields all 3.
        assert_eq!(
            prs.len(),
            3,
            "expected PRs from all pages, got {}",
            prs.len()
        );
    }

    // --- parse_timeline_events ---

    fn ts(s: &str) -> chrono::DateTime<chrono::Utc> {
        s.parse().unwrap()
    }

    // LOCKED: regression for since-my-review review workflow (feat/since-my-review).
    // Timeline committed events must yield last_commit_at.
    #[test]
    fn timeline_extracts_commit_dates() {
        let events = vec![
            json!({
                "event": "committed",
                "committer": { "name": "a", "email": "a@b.c", "date": "2026-09-01T10:00:00Z" }
            }),
            json!({
                "event": "committed",
                "committer": { "name": "a", "email": "a@b.c", "date": "2026-09-03T10:00:00Z" }
            }),
        ];
        let signals = parse_timeline_events(&events, Some("me"));
        assert_eq!(signals.last_commit_at, Some(ts("2026-09-03T10:00:00Z")));
    }

    // LOCKED: regression for since-my-review review workflow (feat/since-my-review).
    // Force-pushes must count as commit activity.
    #[test]
    fn timeline_force_push_counts_as_commit_activity() {
        let events = vec![
            json!({
                "event": "committed",
                "committer": { "name": "a", "email": "a@b.c", "date": "2026-08-01T10:00:00Z" }
            }),
            // Rebase pushed old commits; the force-push event is newer.
            json!({
                "event": "head_ref_force_pushed",
                "actor": { "login": "author" },
                "created_at": "2026-09-05T10:00:00Z"
            }),
        ];
        let signals = parse_timeline_events(&events, Some("me"));
        assert_eq!(signals.last_commit_at, Some(ts("2026-09-05T10:00:00Z")));
    }

    // LOCKED: regression for committer-date vs push-time (pr-pal#2 Copilot review).
    // Old local commits pushed after my review keep old committer dates; the
    // timeline stream order (commit event after my reviewed event) must flag it.
    #[test]
    fn timeline_flags_commit_appearing_after_my_review() {
        let events = vec![
            json!({
                "event": "reviewed",
                "user": { "login": "me" },
                "submitted_at": "2026-09-10T10:00:00Z"
            }),
            // Committed days before the review, pushed after it.
            json!({
                "event": "committed",
                "committer": { "name": "a", "email": "a@b.c", "date": "2026-09-01T10:00:00Z" }
            }),
        ];
        let signals = parse_timeline_events(&events, Some("me"));
        assert!(signals.commit_after_my_activity);
    }

    // LOCKED: regression for committer-date vs push-time (pr-pal#2 Copilot review).
    // Commit before my review in the stream: ball stays with the author.
    #[test]
    fn timeline_does_not_flag_commit_before_my_review() {
        let events = vec![
            json!({
                "event": "committed",
                "committer": { "name": "a", "email": "a@b.c", "date": "2026-09-01T10:00:00Z" }
            }),
            json!({
                "event": "reviewed",
                "user": { "login": "me" },
                "submitted_at": "2026-09-10T10:00:00Z"
            }),
        ];
        let signals = parse_timeline_events(&events, Some("me"));
        assert!(!signals.commit_after_my_activity);
    }

    // LOCKED: regression for committer-date vs push-time (pr-pal#2 Copilot review).
    // My comment after a late push re-arms the cycle: flag clears.
    #[test]
    fn timeline_my_comment_after_late_push_clears_flag() {
        let events = vec![
            json!({
                "event": "reviewed",
                "user": { "login": "me" },
                "submitted_at": "2026-09-10T10:00:00Z"
            }),
            json!({
                "event": "committed",
                "committer": { "name": "a", "email": "a@b.c", "date": "2026-09-01T10:00:00Z" }
            }),
            json!({
                "event": "commented",
                "actor": { "login": "me" },
                "created_at": "2026-09-12T10:00:00Z"
            }),
        ];
        let signals = parse_timeline_events(&events, Some("me"));
        assert!(!signals.commit_after_my_activity);
    }

    // LOCKED: regression for committer-date vs push-time (pr-pal#2 Copilot review).
    // Someone else's review does not gate the stream-order flag.
    #[test]
    fn timeline_ignores_other_reviewers_for_stream_order() {
        let events = vec![
            json!({
                "event": "reviewed",
                "user": { "login": "someone-else" },
                "submitted_at": "2026-09-10T10:00:00Z"
            }),
            json!({
                "event": "committed",
                "committer": { "name": "a", "email": "a@b.c", "date": "2026-09-01T10:00:00Z" }
            }),
        ];
        let signals = parse_timeline_events(&events, Some("me"));
        assert!(!signals.commit_after_my_activity);
    }

    // LOCKED: regression for since-my-review review workflow (feat/since-my-review).
    // Only my mentions count, matched case-insensitively.
    #[test]
    fn timeline_extracts_mentions_of_user_only() {
        let events = vec![
            json!({
                "event": "mentioned",
                "actor": { "login": "someone-else" },
                "created_at": "2026-09-06T10:00:00Z"
            }),
            json!({
                "event": "mentioned",
                "actor": { "login": "Me" },
                "created_at": "2026-09-04T10:00:00Z"
            }),
        ];
        let signals = parse_timeline_events(&events, Some("me"));
        assert_eq!(signals.mentioned_at, Some(ts("2026-09-04T10:00:00Z")));
    }

    // LOCKED: regression for since-my-review review workflow (feat/since-my-review).
    // Only re-requests aimed at me count.
    #[test]
    fn timeline_extracts_review_requests_for_user_only() {
        let events = vec![
            json!({
                "event": "review_requested",
                "requested_reviewer": { "login": "someone-else" },
                "created_at": "2026-09-06T10:00:00Z"
            }),
            json!({
                "event": "review_requested",
                "requested_reviewer": { "login": "me" },
                "created_at": "2026-09-05T10:00:00Z"
            }),
        ];
        let signals = parse_timeline_events(&events, Some("me"));
        assert_eq!(
            signals.review_requested_at,
            Some(ts("2026-09-05T10:00:00Z"))
        );
    }

    // LOCKED: regression for since-my-review review workflow (feat/since-my-review).
    // Only my comments move the activity anchor.
    #[test]
    fn timeline_extracts_my_comments_only() {
        let events = vec![
            json!({
                "event": "commented",
                "actor": { "login": "me" },
                "created_at": "2026-09-02T10:00:00Z"
            }),
            json!({
                "event": "commented",
                "actor": { "login": "someone-else" },
                "created_at": "2026-09-06T10:00:00Z"
            }),
        ];
        let signals = parse_timeline_events(&events, Some("me"));
        assert_eq!(signals.my_last_comment_at, Some(ts("2026-09-02T10:00:00Z")));
    }

    // LOCKED: regression for since-my-review review workflow (feat/since-my-review).
    // Malformed timeline entries must be skipped, not crash.
    #[test]
    fn timeline_ignores_unknown_events_and_missing_fields() {
        let events = vec![
            json!({ "event": "labeled", "created_at": "2026-09-06T10:00:00Z" }),
            json!({ "event": "committed" }),
            json!({ "event": "mentioned", "created_at": "2026-09-06T10:00:00Z" }),
            json!({}),
        ];
        let signals = parse_timeline_events(&events, Some("me"));
        assert_eq!(signals, TimelineSignals::default());
    }

    // LOCKED: regression for since-my-review review workflow (feat/since-my-review).
    // No auth username: commits still tracked, user signals off.
    #[test]
    fn timeline_without_username_still_tracks_commits() {
        let events = vec![
            json!({
                "event": "committed",
                "committer": { "name": "a", "email": "a@b.c", "date": "2026-09-01T10:00:00Z" }
            }),
            json!({
                "event": "mentioned",
                "actor": { "login": "me" },
                "created_at": "2026-09-02T10:00:00Z"
            }),
        ];
        let signals = parse_timeline_events(&events, None);
        assert_eq!(signals.last_commit_at, Some(ts("2026-09-01T10:00:00Z")));
        assert_eq!(signals.mentioned_at, None);
    }

    // --- enrichment wiring for review signals ---

    fn pull_details_json(number: u64) -> serde_json::Value {
        json!({
            "id": number,
            "number": number,
            "url": format!("https://api.github.com/repos/o/r/pulls/{}", number),
            "head": { "ref": "feature", "sha": "abc123" },
            "base": { "ref": "main", "sha": "def456" },
            "locked": false,
            "state": "open",
            "additions": 4,
            "deletions": 2,
            "draft": false
        })
    }

    fn reviews_json() -> serde_json::Value {
        json!([
            {
                "id": 900,
                "node_id": "R_900",
                "html_url": "https://github.com/o/r/pull/5#pullrequestreview-900",
                "user": author_json("me"),
                "state": "APPROVED",
                "submitted_at": "2026-09-01T00:00:00Z"
            },
            {
                "id": 901,
                "node_id": "R_901",
                "html_url": "https://github.com/o/r/pull/5#pullrequestreview-901",
                "user": author_json("me"),
                "state": "COMMENTED",
                "submitted_at": "2026-09-02T00:00:00Z"
            },
            {
                "id": 902,
                "node_id": "R_902",
                "html_url": "https://github.com/o/r/pull/5#pullrequestreview-902",
                "user": author_json("someone-else"),
                "state": "APPROVED",
                "submitted_at": "2026-09-03T00:00:00Z"
            }
        ])
    }

    async fn mount_common_enrichment_mocks(server: &MockServer) {
        Mock::given(method("GET"))
            .and(path("/search/issues"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(search_body(vec![pr_issue_json(5, "o", "r")])),
            )
            .mount(server)
            .await;
        Mock::given(method("GET"))
            .and(path("/repos/o/r/pulls/5"))
            .respond_with(ResponseTemplate::new(200).set_body_json(pull_details_json(5)))
            .mount(server)
            .await;
        Mock::given(method("GET"))
            .and(path("/repos/o/r/pulls/5/reviews"))
            .respond_with(ResponseTemplate::new(200).set_body_json(reviews_json()))
            .mount(server)
            .await;
    }

    // LOCKED: regression for since-my-review review workflow (feat/since-my-review).
    // Enrichment must populate signals via paginated timeline.
    #[tokio::test]
    async fn enrichment_populates_review_signals_from_timeline() {
        let server = MockServer::start().await;
        mount_common_enrichment_mocks(&server).await;

        // Two timeline pages to prove pagination: a full page of noise, then
        // the interesting events.
        let full_page: Vec<serde_json::Value> = (0..100)
            .map(|_| json!({ "event": "labeled", "created_at": "2026-08-01T00:00:00Z" }))
            .collect();
        Mock::given(method("GET"))
            .and(path("/repos/o/r/issues/5/timeline"))
            .and(query_param("page", "1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(full_page))
            .mount(&server)
            .await;
        let page2 = json!([
            {
                "event": "committed",
                "committer": { "name": "a", "email": "a@b.c", "date": "2026-09-04T00:00:00Z" }
            },
            {
                "event": "commented",
                "actor": { "login": "me" },
                "created_at": "2026-09-05T00:00:00Z"
            },
            {
                "event": "mentioned",
                "actor": { "login": "me" },
                "created_at": "2026-09-06T00:00:00Z"
            },
            {
                "event": "review_requested",
                "requested_reviewer": { "login": "me" },
                "created_at": "2026-09-07T00:00:00Z"
            }
        ]);
        Mock::given(method("GET"))
            .and(path("/repos/o/r/issues/5/timeline"))
            .and(query_param("page", "2"))
            .respond_with(ResponseTemplate::new(200).set_body_json(page2))
            .mount(&server)
            .await;

        let client = Octocrab::builder()
            .base_uri(server.uri())
            .unwrap()
            .build()
            .unwrap();

        let prs = search_and_enrich_prs(&client, "review-requested:@me", Some("me"), None, true)
            .await
            .expect("search_and_enrich_prs should succeed");

        assert_eq!(prs.len(), 1);
        let pr = &prs[0];
        assert_eq!(pr.approvals, 2);
        assert!(pr.user_has_reviewed);
        assert_eq!(
            pr.signals.my_last_review_at,
            Some(ts("2026-09-02T00:00:00Z"))
        );
        assert_eq!(
            pr.signals.my_last_comment_at,
            Some(ts("2026-09-05T00:00:00Z"))
        );
        assert_eq!(pr.signals.last_commit_at, Some(ts("2026-09-04T00:00:00Z")));
        assert_eq!(pr.signals.mentioned_at, Some(ts("2026-09-06T00:00:00Z")));
        assert_eq!(
            pr.signals.review_requested_at,
            Some(ts("2026-09-07T00:00:00Z"))
        );
    }

    // LOCKED: regression for review-fetch pagination (pr-pal#2 Copilot review).
    // fetch_pr_reviews must follow the Link header; reviews beyond the first
    // page still count toward approvals and the my-review anchor.
    #[tokio::test]
    async fn enrichment_counts_reviews_from_all_pages() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/search/issues"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(search_body(vec![pr_issue_json(5, "o", "r")])),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/repos/o/r/pulls/5"))
            .respond_with(ResponseTemplate::new(200).set_body_json(pull_details_json(5)))
            .mount(&server)
            .await;

        // Page 2: my review and a second approval live beyond page 1.
        Mock::given(method("GET"))
            .and(path("/repos/o/r/pulls/5/reviews"))
            .and(query_param("page", "2"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([
                {
                    "id": 903,
                    "node_id": "R_903",
                    "html_url": "https://github.com/o/r/pull/5#pullrequestreview-903",
                    "user": author_json("me"),
                    "state": "APPROVED",
                    "submitted_at": "2026-09-05T00:00:00Z"
                }
            ])))
            .mount(&server)
            .await;

        // Page 1: someone else's approval plus a Link header to page 2.
        let next_link = format!(
            "<{}/repos/o/r/pulls/5/reviews?per_page=100&page=2>; rel=\"next\"",
            server.uri()
        );
        Mock::given(method("GET"))
            .and(path("/repos/o/r/pulls/5/reviews"))
            .and(query_param_is_missing("page"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("link", next_link.as_str())
                    .set_body_json(json!([
                        {
                            "id": 902,
                            "node_id": "R_902",
                            "html_url": "https://github.com/o/r/pull/5#pullrequestreview-902",
                            "user": author_json("someone-else"),
                            "state": "APPROVED",
                            "submitted_at": "2026-09-03T00:00:00Z"
                        }
                    ])),
            )
            .mount(&server)
            .await;

        let client = Octocrab::builder()
            .base_uri(server.uri())
            .unwrap()
            .build()
            .unwrap();

        let prs = search_and_enrich_prs(&client, "review-requested:@me", Some("me"), None, false)
            .await
            .expect("search_and_enrich_prs should succeed");

        assert_eq!(prs.len(), 1);
        let pr = &prs[0];
        assert_eq!(pr.approvals, 2, "approvals must include page 2");
        assert!(pr.user_has_reviewed, "my review is on page 2");
        assert_eq!(
            pr.signals.my_last_review_at,
            Some(ts("2026-09-05T00:00:00Z"))
        );
    }

    // LOCKED: regression for timeline fetch failure (pr-pal#2 Copilot review).
    // A failed timeline fetch must fail open: clear the review anchor so the
    // PR stays active instead of being suppressed as awaiting-author.
    #[tokio::test]
    async fn enrichment_fails_open_when_timeline_errors() {
        let server = MockServer::start().await;
        mount_common_enrichment_mocks(&server).await;

        Mock::given(method("GET"))
            .and(path("/repos/o/r/issues/5/timeline"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let client = Octocrab::builder()
            .base_uri(server.uri())
            .unwrap()
            .build()
            .unwrap();

        let prs = search_and_enrich_prs(&client, "review-requested:@me", Some("me"), None, true)
            .await
            .expect("search_and_enrich_prs should succeed");

        assert_eq!(prs.len(), 1);
        let pr = &prs[0];
        assert!(pr.user_has_reviewed, "legacy scoring signal is preserved");
        assert_eq!(
            pr.signals.my_last_review_at, None,
            "anchor must clear so the PR is not suppressed on API failure"
        );
    }

    // LOCKED: regression for swallowed timeline rate limit (pr-pal#2 Copilot review).
    // A rate-limited timeline fetch must trip the shared stop flag so the
    // remaining PRs skip enrichment; ordinary errors keep failing open
    // (covered by enrichment_fails_open_when_timeline_errors).
    #[tokio::test]
    async fn enrichment_rate_limited_timeline_trips_stop_flag() {
        let server = MockServer::start().await;
        mount_common_enrichment_mocks(&server).await;

        Mock::given(method("GET"))
            .and(path("/repos/o/r/issues/5/timeline"))
            .respond_with(ResponseTemplate::new(403).set_body_json(json!({
                "message": "API rate limit exceeded for user",
                "documentation_url": "https://docs.github.com/rest/overview/rate-limits"
            })))
            .mount(&server)
            .await;

        let client = Octocrab::builder()
            .base_uri(server.uri())
            .unwrap()
            .build()
            .unwrap();

        let pr = PullRequest {
            title: "t".to_string(),
            number: 5,
            author: "a".to_string(),
            repo: "o/r".to_string(),
            url: "https://github.com/o/r/pull/5".to_string(),
            created_at: ts("2026-01-01T00:00:00Z"),
            updated_at: ts("2026-01-01T00:00:00Z"),
            additions: 0,
            deletions: 0,
            approvals: 0,
            draft: false,
            labels: vec![],
            user_has_reviewed: false,
            filtered_size: None,
            signals: Default::default(),
        };

        let rate_limited = Arc::new(AtomicBool::new(false));
        let enriched = enrich_pr_with_rate_limit_check(
            client,
            pr,
            Arc::clone(&rate_limited),
            Some("me".to_string()),
            None,
            true,
        )
        .await;

        assert!(
            rate_limited.load(Ordering::Relaxed),
            "rate-limited timeline fetch must set the stop flag"
        );
        assert_eq!(
            enriched.signals.my_last_review_at, None,
            "anchor still clears so partial results fail open"
        );
    }

    // LOCKED: regression for 403-as-rate-limit misclassification (pr-pal#2 Copilot review).
    // "403" appearing incidentally in an error chain (URLs containing PR
    // number 403, quoted status lines) is not evidence of rate limiting;
    // only the explicit rate-limit message or HTTP 429 is.
    #[test]
    fn rate_limit_predicate_ignores_incidental_403_text() {
        let err = anyhow!(
            "error sending request for url (https://api.github.com/repos/o/r/issues/403/timeline)"
        );
        assert!(!is_rate_limit_error(&err));

        let err = anyhow!("API rate limit exceeded for user");
        assert!(is_rate_limit_error(&err));
    }

    // LOCKED: regression for 403-as-rate-limit misclassification (pr-pal#2 Copilot review).
    // A plain forbidden response (permissions, SAML, integration scope) is
    // not a rate limit: it must fail open for that PR only, leaving the
    // shared stop flag untouched so remaining PRs still enrich.
    #[tokio::test]
    async fn enrichment_plain_403_does_not_trip_stop_flag() {
        let server = MockServer::start().await;
        mount_common_enrichment_mocks(&server).await;

        Mock::given(method("GET"))
            .and(path("/repos/o/r/issues/5/timeline"))
            .respond_with(ResponseTemplate::new(403).set_body_json(json!({
                "message": "Resource not accessible by personal access token",
                "documentation_url": "https://docs.github.com/rest"
            })))
            .mount(&server)
            .await;

        let client = Octocrab::builder()
            .base_uri(server.uri())
            .unwrap()
            .build()
            .unwrap();

        let pr = PullRequest {
            title: "t".to_string(),
            number: 5,
            author: "a".to_string(),
            repo: "o/r".to_string(),
            url: "https://github.com/o/r/pull/5".to_string(),
            created_at: ts("2026-01-01T00:00:00Z"),
            updated_at: ts("2026-01-01T00:00:00Z"),
            additions: 0,
            deletions: 0,
            approvals: 0,
            draft: false,
            labels: vec![],
            user_has_reviewed: false,
            filtered_size: None,
            signals: Default::default(),
        };

        let rate_limited = Arc::new(AtomicBool::new(false));
        let enriched = enrich_pr_with_rate_limit_check(
            client,
            pr,
            Arc::clone(&rate_limited),
            Some("me".to_string()),
            None,
            true,
        )
        .await;

        assert!(
            !rate_limited.load(Ordering::Relaxed),
            "plain 403 must not be treated as a rate limit"
        );
        assert_eq!(
            enriched.signals.my_last_review_at, None,
            "affected PR still fails open"
        );
    }

    // LOCKED: regression for since-my-review review workflow (feat/since-my-review).
    // No timeline API calls unless the feature is configured.
    #[tokio::test]
    async fn enrichment_skips_timeline_when_signals_not_needed() {
        let server = MockServer::start().await;
        mount_common_enrichment_mocks(&server).await;

        // No timeline mock mounted: a timeline call would 404 and, more to
        // the point, need_signals=false must not even attempt it.
        Mock::given(method("GET"))
            .and(path("/repos/o/r/issues/5/timeline"))
            .respond_with(ResponseTemplate::new(500))
            .expect(0)
            .mount(&server)
            .await;

        let client = Octocrab::builder()
            .base_uri(server.uri())
            .unwrap()
            .build()
            .unwrap();

        let prs = search_and_enrich_prs(&client, "review-requested:@me", Some("me"), None, false)
            .await
            .expect("search_and_enrich_prs should succeed");

        assert_eq!(prs.len(), 1);
        let pr = &prs[0];
        assert!(pr.user_has_reviewed);
        // Review timestamp comes from the reviews call either way; timeline
        // signals stay unset.
        assert_eq!(
            pr.signals.my_last_review_at,
            Some(ts("2026-09-02T00:00:00Z"))
        );
        assert_eq!(pr.signals.last_commit_at, None);
        assert_eq!(pr.signals.mentioned_at, None);
    }
}
