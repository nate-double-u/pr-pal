use chrono::Duration;
use owo_colors::OwoColorize;
use std::io::IsTerminal;
use terminal_size::{terminal_size, Width};

use crate::github::types::PullRequest;

/// Format a list of PRs as one line per PR
/// Format: "{title} | {repo} | {author} | {url}"
pub fn format_pr_list(prs: &[PullRequest], use_colors: bool) -> String {
    if prs.is_empty() {
        return "No pull requests found.".to_string();
    }

    prs.iter()
        .map(|pr| format_pr_line(pr, use_colors))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Format a single PR as one line
fn format_pr_line(pr: &PullRequest, use_colors: bool) -> String {
    if use_colors {
        format!(
            "{} | {} | {} | {}",
            pr.title.bold(),
            pr.repo.cyan(),
            pr.author.yellow(),
            pr.short_ref().underline()
        )
    } else {
        format!(
            "{} | {} | {} | {}",
            pr.title,
            pr.repo,
            pr.author,
            pr.short_ref()
        )
    }
}

/// Format a single PR with detailed multi-line output (for verbose mode)
pub fn format_pr_detail(pr: &PullRequest, use_colors: bool) -> String {
    let age = format_age(pr.age());
    let total_size = pr.size();

    if use_colors {
        format!(
            "{}\n  Repo: {}\n  Author: {}\n  Age: {}\n  Size: +{}/{} ({} lines)\n  Approvals: {}\n  URL: {}",
            pr.title.bold(),
            pr.repo.cyan(),
            pr.author.yellow(),
            age,
            pr.additions.green(),
            pr.deletions.red(),
            total_size,
            pr.approvals,
            pr.url.underline()
        )
    } else {
        format!(
            "{}\n  Repo: {}\n  Author: {}\n  Age: {}\n  Size: +{}/{} ({} lines)\n  Approvals: {}\n  URL: {}",
            pr.title,
            pr.repo,
            pr.author,
            age,
            pr.additions,
            pr.deletions,
            total_size,
            pr.approvals,
            pr.url
        )
    }
}

/// Check if stdout is a TTY (for auto-detecting color support)
pub fn should_use_colors() -> bool {
    std::io::stdout().is_terminal()
}

/// Format a score in compact notation (1.5k, 2.3M, 847)
/// If incomplete is true, appends asterisk to indicate partial scoring
pub fn format_score(score: f64, incomplete: bool) -> String {
    let formatted = if score >= 1_000_000.0 {
        format!("{:.1}M", score / 1_000_000.0)
    } else if score >= 1_000.0 {
        format!("{:.1}k", score / 1_000.0)
    } else {
        format!("{:.0}", score)
    };

    // Trim trailing .0 (e.g., "1.0k" -> "1k")
    let trimmed = formatted.replace(".0M", "M").replace(".0k", "k");

    if incomplete {
        format!("{}*", trimmed)
    } else {
        trimmed
    }
}

/// A PR with its calculated score for display
pub struct ScoredPr<'a> {
    pub pr: &'a PullRequest,
    pub score: f64,
    pub incomplete: bool,
}

/// Get terminal width, defaulting to None for pipes (unlimited)
fn get_terminal_width() -> Option<usize> {
    terminal_size().map(|(Width(w), _)| w as usize)
}

/// Truncate title to fit available width, accounting for Unicode
fn truncate_title(title: &str, max_width: usize) -> String {
    let chars: Vec<char> = title.chars().collect();
    if chars.len() <= max_width {
        title.to_string()
    } else if max_width > 3 {
        format!("{}...", chars[..max_width - 3].iter().collect::<String>())
    } else {
        chars[..max_width].iter().collect()
    }
}

/// Format PRs as scored table with columns: Index, Score, Title, URL
/// No headers (minimal format per CONTEXT.md)
/// Index column: 3 chars (fits "99."), right-aligned
/// Score column is right-aligned, 7 chars wide (fits "9999.9M")
pub fn format_scored_table(prs: &[ScoredPr], use_colors: bool) -> String {
    if prs.is_empty() {
        return "No pull requests found.".to_string();
    }

    let term_width = get_terminal_width();

    // Index column: 3 chars + 1 space = 4
    // Score column: 7 chars + 2 spaces = 9
    // URL: varies, ~50 chars typical for GitHub
    // Leave rest for title
    let index_width = 3;
    let score_width = 7;
    let separator = "  ";

    prs.iter()
        .enumerate()
        .map(|(idx, scored)| {
            // 1-based index, right-aligned with trailing dot
            let index_str = format!("{:>2}.", idx + 1);
            let score_str = format_score(scored.score, scored.incomplete);
            let score_padded = format!("{:>width$}", score_str, width = score_width);

            // Calculate available title width (accounting for index column)
            let ref_len = scored.pr.short_ref().len();
            let fixed_width = index_width + 1 + score_width + separator.len() * 2 + ref_len;

            let title = if let Some(width) = term_width {
                if width > fixed_width + 10 {
                    truncate_title(&scored.pr.title, width - fixed_width)
                } else {
                    // Very narrow terminal, show truncated
                    truncate_title(&scored.pr.title, 20)
                }
            } else {
                // No terminal (pipe), don't truncate
                scored.pr.title.clone()
            };

            if use_colors {
                format!(
                    "{} {}{}{}{}{}",
                    index_str.dimmed(),
                    score_padded.bold(),
                    separator,
                    title,
                    separator,
                    scored.pr.short_ref().underline()
                )
            } else {
                format!(
                    "{} {}{}{}{}{}",
                    index_str,
                    score_padded,
                    separator,
                    title,
                    separator,
                    scored.pr.short_ref()
                )
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Format PRs as tab-separated values for scripting
/// Columns: score, title, repo, pr_ref (no headers, no colors)
pub fn format_tsv(prs: &[ScoredPr]) -> String {
    if prs.is_empty() {
        return String::new();
    }

    prs.iter()
        .map(|scored| {
            let score = scored.score.round() as i64;
            format!(
                "{}\t{}\t{}\t{}",
                score,
                scored.pr.title,
                scored.pr.repo,
                scored.pr.short_ref()
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Format a duration into a human-readable age string
/// "2h" for hours, "3d" for days, "1w" for weeks
pub fn format_age(duration: Duration) -> String {
    let hours = duration.num_hours();
    let days = duration.num_days();
    let weeks = days / 7;

    if weeks >= 1 {
        format!("{}w", weeks)
    } else if days >= 1 {
        format!("{}d", days)
    } else if hours >= 1 {
        format!("{}h", hours)
    } else {
        let minutes = duration.num_minutes();
        if minutes >= 1 {
            format!("{}m", minutes)
        } else {
            "now".to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn sample_pr() -> PullRequest {
        PullRequest {
            title: "Fix login bug".to_string(),
            number: 123,
            author: "octocat".to_string(),
            repo: "owner/repo".to_string(),
            url: "https://github.com/owner/repo/pull/123".to_string(),
            created_at: Utc::now() - Duration::hours(5),
            updated_at: Utc::now() - Duration::hours(1),
            additions: 50,
            deletions: 10,
            approvals: 1,
            draft: false,
            labels: vec![],
            user_has_reviewed: false,
            filtered_size: None,
            signals: Default::default(),
        }
    }

    #[test]
    fn test_format_pr_list_empty() {
        let prs: Vec<PullRequest> = vec![];
        let result = format_pr_list(&prs, false);
        assert_eq!(result, "No pull requests found.");
    }

    #[test]
    fn test_format_pr_list_single() {
        let prs = vec![sample_pr()];
        let result = format_pr_list(&prs, false);
        assert!(result.contains("Fix login bug"));
        assert!(result.contains("owner/repo"));
        assert!(result.contains("octocat"));
        assert!(result.contains("owner/repo#123"));
    }

    #[test]
    fn test_format_pr_detail() {
        let pr = sample_pr();
        let result = format_pr_detail(&pr, false);
        assert!(result.contains("Fix login bug"));
        assert!(result.contains("Repo: owner/repo"));
        assert!(result.contains("Author: octocat"));
        assert!(result.contains("Size: +50/10 (60 lines)"));
        assert!(result.contains("Approvals: 1"));
    }

    #[test]
    fn test_format_age_hours() {
        let duration = Duration::hours(3);
        assert_eq!(format_age(duration), "3h");
    }

    #[test]
    fn test_format_age_days() {
        let duration = Duration::days(2);
        assert_eq!(format_age(duration), "2d");
    }

    #[test]
    fn test_format_age_weeks() {
        let duration = Duration::weeks(2);
        assert_eq!(format_age(duration), "2w");
    }

    #[test]
    fn test_format_age_minutes() {
        let duration = Duration::minutes(30);
        assert_eq!(format_age(duration), "30m");
    }

    #[test]
    fn test_format_age_now() {
        let duration = Duration::seconds(30);
        assert_eq!(format_age(duration), "now");
    }

    // format_score tests
    #[test]
    fn test_format_score_small() {
        assert_eq!(format_score(500.0, false), "500");
    }

    #[test]
    fn test_format_score_zero() {
        assert_eq!(format_score(0.0, false), "0");
    }

    #[test]
    fn test_format_score_thousand_exact() {
        assert_eq!(format_score(1000.0, false), "1k");
    }

    #[test]
    fn test_format_score_thousand_decimal() {
        assert_eq!(format_score(1500.0, false), "1.5k");
    }

    #[test]
    fn test_format_score_million_exact() {
        assert_eq!(format_score(1_000_000.0, false), "1M");
    }

    #[test]
    fn test_format_score_million_decimal() {
        assert_eq!(format_score(2_300_000.0, false), "2.3M");
    }

    #[test]
    fn test_format_score_with_incomplete() {
        assert_eq!(format_score(1500.0, true), "1.5k*");
    }

    #[test]
    fn test_format_score_small_with_incomplete() {
        assert_eq!(format_score(847.0, true), "847*");
    }

    // truncate_title tests
    #[test]
    fn test_truncate_title_short() {
        assert_eq!(truncate_title("Short title", 20), "Short title");
    }

    #[test]
    fn test_truncate_title_exact() {
        assert_eq!(truncate_title("Exact", 5), "Exact");
    }

    #[test]
    fn test_truncate_title_long() {
        assert_eq!(
            truncate_title("This is a very long title", 15),
            "This is a ve..."
        );
    }

    #[test]
    fn test_truncate_title_unicode() {
        // Unicode characters should be handled correctly (by char, not by byte)
        assert_eq!(truncate_title("Hello cafe", 10), "Hello cafe");
        assert_eq!(truncate_title("Hello cafe world", 10), "Hello c...");
    }

    #[test]
    fn test_truncate_title_very_narrow() {
        // Very narrow case (max_width <= 3)
        assert_eq!(truncate_title("Hello world", 3), "Hel");
    }

    // format_scored_table tests
    #[test]
    fn test_format_scored_table_empty() {
        let prs: Vec<ScoredPr> = vec![];
        let result = format_scored_table(&prs, false);
        assert_eq!(result, "No pull requests found.");
    }

    #[test]
    fn test_format_scored_table_single() {
        let pr = sample_pr();
        let scored_prs = vec![ScoredPr {
            pr: &pr,
            score: 1500.0,
            incomplete: false,
        }];
        let result = format_scored_table(&scored_prs, false);
        // Index should be 1-based
        assert!(result.contains(" 1."));
        // Score should be right-aligned in 7-char column
        assert!(result.contains("1.5k"));
        assert!(result.contains("Fix login bug"));
        assert!(result.contains("owner/repo#123"));
    }

    #[test]
    fn test_format_scored_table_incomplete() {
        let pr = sample_pr();
        let scored_prs = vec![ScoredPr {
            pr: &pr,
            score: 847.0,
            incomplete: true,
        }];
        let result = format_scored_table(&scored_prs, false);
        assert!(result.contains(" 1."));
        assert!(result.contains("847*"));
    }

    #[test]
    fn test_format_scored_table_multiple() {
        let pr1 = sample_pr();
        let mut pr2 = sample_pr();
        pr2.title = "Add new feature".to_string();
        pr2.number = 456;
        pr2.url = "https://github.com/owner/repo/pull/456".to_string();

        let scored_prs = vec![
            ScoredPr {
                pr: &pr1,
                score: 2000.0,
                incomplete: false,
            },
            ScoredPr {
                pr: &pr2,
                score: 500.0,
                incomplete: false,
            },
        ];
        let result = format_scored_table(&scored_prs, false);
        let lines: Vec<&str> = result.lines().collect();
        assert_eq!(lines.len(), 2);
        // Check indices are sequential
        assert!(lines[0].contains(" 1."));
        assert!(lines[1].contains(" 2."));
        // Check scores and titles
        assert!(lines[0].contains("2k"));
        assert!(lines[0].contains("Fix login bug"));
        assert!(lines[1].contains("500"));
        assert!(lines[1].contains("Add new feature"));
    }

    // format_tsv tests
    #[test]
    fn test_format_tsv_empty() {
        let prs: Vec<ScoredPr> = vec![];
        let result = format_tsv(&prs);
        assert_eq!(result, "");
    }

    #[test]
    fn test_format_tsv_single() {
        let pr = sample_pr();
        let scored_prs = vec![ScoredPr {
            pr: &pr,
            score: 1500.7,
            incomplete: false,
        }];
        let result = format_tsv(&scored_prs);
        assert_eq!(result, "1501\tFix login bug\towner/repo\towner/repo#123");
    }

    #[test]
    fn test_format_tsv_multiple() {
        let pr1 = sample_pr();
        let mut pr2 = sample_pr();
        pr2.title = "Add feature".to_string();
        pr2.url = "https://github.com/owner/repo/pull/456".to_string();

        let scored_prs = vec![
            ScoredPr {
                pr: &pr1,
                score: 2000.0,
                incomplete: false,
            },
            ScoredPr {
                pr: &pr2,
                score: 500.0,
                incomplete: true,
            },
        ];
        let result = format_tsv(&scored_prs);
        let lines: Vec<&str> = result.lines().collect();
        assert_eq!(lines.len(), 2);
        // Verify tab-separated format
        assert!(lines[0].contains('\t'));
        assert_eq!(lines[0].split('\t').count(), 4);
        assert!(lines[0].starts_with("2000\t"));
        assert!(lines[1].starts_with("500\t"));
    }

    #[test]
    fn test_format_scored_table_index_format() {
        // Verify index format: right-aligned, 1-based, with trailing dot
        let pr = sample_pr();
        let scored_prs = vec![ScoredPr {
            pr: &pr,
            score: 100.0,
            incomplete: false,
        }];
        let result = format_scored_table(&scored_prs, false);
        // Should start with " 1." (space for alignment, then index)
        assert!(result.starts_with(" 1."));
    }
}
