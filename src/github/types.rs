use chrono::{DateTime, Utc};

use crate::review_state::ReviewSignals;

#[derive(Debug, Clone)]
pub struct PullRequest {
    pub title: String,
    pub number: u64,
    pub author: String,
    pub repo: String, // "owner/repo" format
    pub url: String,  // HTML URL for browser
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub additions: u64, // Lines added
    pub deletions: u64, // Lines deleted
    pub approvals: u32, // Approval count (will need separate API call)
    pub draft: bool,
    pub labels: Vec<String>,        // GitHub label names on this PR
    pub user_has_reviewed: bool,    // Whether the authenticated user has submitted a review
    pub filtered_size: Option<u64>, // Size after applying exclude patterns (if configured)
    pub signals: ReviewSignals,     // Review-cycle timestamps (populated by enrichment)
}

impl PullRequest {
    /// Calculate PR age from creation time
    pub fn age(&self) -> chrono::Duration {
        Utc::now() - self.created_at
    }

    /// Calculate total size, using filtered size if available (exclude patterns applied)
    pub fn size(&self) -> u64 {
        self.filtered_size
            .unwrap_or(self.additions + self.deletions)
    }

    /// Return a short reference in the format "owner/repo#123"
    pub fn short_ref(&self) -> String {
        format!("{}#{}", self.repo, self.number)
    }
}
