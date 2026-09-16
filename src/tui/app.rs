use crate::config::Config;
use crate::github::cache::{CacheConfig, DiskCache};
use crate::github::types::PullRequest;
use crate::hide::HidePaths;
use crate::ignore::IgnoreState;
use crate::review_state::{review_state, ReviewState};
use crate::scoring::ScoreResult;
use crate::snooze::SnoozeState;
use crate::snooze::SuppressPolicy;
use crate::tui::theme::{Theme, ThemeColors};
use chrono::{DateTime, Utc};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::Instant;

const MAX_UNDO: usize = 50;

/// Merge manually snoozed and suppressed (awaiting author) PRs into one
/// display list, sorted by score descending (ties: older PR first). Returns
/// the merged list and the set of suppressed URLs for tagging.
pub fn merge_snoozed_lists(
    snoozed: Vec<(PullRequest, ScoreResult)>,
    suppressed: Vec<(PullRequest, ScoreResult)>,
) -> (Vec<(PullRequest, ScoreResult)>, HashSet<String>) {
    let suppressed_urls: HashSet<String> =
        suppressed.iter().map(|(pr, _)| pr.url.clone()).collect();
    let mut merged = snoozed;
    merged.extend(suppressed);
    merged.sort_by(|a, b| {
        b.1.score
            .partial_cmp(&a.1.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.created_at.cmp(&b.0.created_at))
    });
    (merged, suppressed_urls)
}

/// Compute the review-cycle state for each PR (keyed by URL), used to tag
/// Active rows with the reason a PR resurfaced. With a suppress policy the
/// tag reflects the effective wake (same rules as partitioning); without one
/// it falls back to the raw review state.
pub fn compute_review_states(
    prs: &[(PullRequest, ScoreResult)],
    policy: Option<&SuppressPolicy>,
    now: DateTime<Utc>,
) -> HashMap<String, ReviewState> {
    prs.iter()
        .map(|(pr, _)| {
            let state = match policy {
                Some(policy) => {
                    crate::snooze::filter::effective_review_state(&pr.signals, policy, now)
                }
                None => review_state(&pr.signals, now, None),
            };
            (pr.url.clone(), state)
        })
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum View {
    Active,
    Snoozed,
}

#[derive(Debug, Clone, PartialEq)]
pub enum InputMode {
    Normal,
    SnoozeInput,
    Help,
    ScoreBreakdown,
}

#[derive(Debug, Clone)]
pub enum UndoAction {
    Snoozed {
        url: String,
        title: String,
        /// The row was suppressed (awaiting author) before the manual snooze;
        /// undo restores suppression instead of activating the row.
        was_suppressed: bool,
    },
    Unsnoozed {
        url: String,
        title: String,
        /// Wake time of the removed snooze; `None` if the row had no entry.
        until: Option<DateTime<Utc>>,
        /// The row resolved to awaiting-author when unsnoozed, so it stayed
        /// in the Snoozed view as suppressed; undo drops the marker instead
        /// of moving rows.
        became_suppressed: bool,
    },
    Resnooze {
        url: String,
        title: String,
        previous_until: DateTime<Utc>,
    },
}

pub struct App {
    pub active_prs: Vec<(PullRequest, ScoreResult)>,
    /// Manually snoozed + suppressed (awaiting author) PRs, merged for display
    pub snoozed_prs: Vec<(PullRequest, ScoreResult)>,
    /// URLs of PRs suppressed as awaiting-author (subset of snoozed_prs)
    pub suppressed_urls: HashSet<String>,
    /// Permanently hidden PRs, oldest ignore first
    pub ignored_prs: Vec<(PullRequest, ScoreResult)>,
    /// Review-cycle state per PR URL, for wake-reason tags
    pub review_states: HashMap<String, ReviewState>,
    pub table_state: ratatui::widgets::TableState,
    pub current_view: View,
    pub snooze_state: SnoozeState,
    pub ignore_state: IgnoreState,
    pub hide_paths: HidePaths,
    pub input_mode: InputMode,
    pub snooze_input: String,
    pub flash_message: Option<(String, Instant)>,
    pub undo_stack: VecDeque<UndoAction>,
    pub last_refresh: Instant,
    pub needs_refresh: bool,
    pub force_refresh: bool,
    pub should_quit: bool,
    pub config: Config,
    pub cache_config: CacheConfig,
    pub cache_handle: Option<Arc<DiskCache>>,
    pub verbose: bool,
    pub is_loading: bool,
    pub spinner_frame: usize,
    pub rate_limit_remaining: Option<u64>,
    pub auth_username: Option<String>,
    pub theme: Theme,
    pub theme_colors: ThemeColors,
    pub last_interaction: Instant,
    /// Data rows visible in the table viewport, set on each render. Page
    /// and viewport jumps fall back to single-row moves before the first
    /// render, when this is still zero.
    pub visible_rows: usize,
}

impl App {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        active_prs: Vec<(PullRequest, ScoreResult)>,
        snoozed_prs: Vec<(PullRequest, ScoreResult)>,
        snooze_state: SnoozeState,
        ignore_state: IgnoreState,
        hide_paths: HidePaths,
        config: Config,
        cache_config: CacheConfig,
        cache_handle: Option<Arc<DiskCache>>,
        verbose: bool,
        auth_username: Option<String>,
        theme: Theme,
    ) -> Self {
        let mut table_state = ratatui::widgets::TableState::default();
        if !active_prs.is_empty() {
            table_state.select(Some(0));
        }

        Self {
            active_prs,
            snoozed_prs,
            suppressed_urls: HashSet::new(),
            ignored_prs: Vec::new(),
            review_states: HashMap::new(),
            table_state,
            current_view: View::Active,
            snooze_state,
            ignore_state,
            hide_paths,
            input_mode: InputMode::Normal,
            snooze_input: String::new(),
            flash_message: None,
            undo_stack: VecDeque::new(),
            last_refresh: Instant::now(),
            needs_refresh: false,
            force_refresh: false,
            should_quit: false,
            config,
            cache_config,
            cache_handle,
            verbose,
            is_loading: false,
            spinner_frame: 0,
            rate_limit_remaining: None,
            auth_username,
            theme,
            theme_colors: ThemeColors::new(theme),
            last_interaction: Instant::now(),
            visible_rows: 0,
        }
    }

    /// Create a new App with empty PR lists in loading state
    /// Used for launching TUI before data arrives
    #[allow(clippy::too_many_arguments)]
    pub fn new_loading(
        snooze_state: SnoozeState,
        ignore_state: IgnoreState,
        hide_paths: HidePaths,
        config: Config,
        cache_config: CacheConfig,
        cache_handle: Option<Arc<DiskCache>>,
        verbose: bool,
        auth_username: Option<String>,
        theme: Theme,
    ) -> Self {
        Self {
            active_prs: Vec::new(),
            snoozed_prs: Vec::new(),
            suppressed_urls: HashSet::new(),
            ignored_prs: Vec::new(),
            review_states: HashMap::new(),
            table_state: ratatui::widgets::TableState::default(),
            current_view: View::Active,
            snooze_state,
            ignore_state,
            hide_paths,
            input_mode: InputMode::Normal,
            snooze_input: String::new(),
            flash_message: None,
            undo_stack: VecDeque::new(),
            last_refresh: Instant::now(),
            needs_refresh: false,
            force_refresh: false,
            should_quit: false,
            config,
            cache_config,
            cache_handle,
            verbose,
            is_loading: true,
            spinner_frame: 0,
            rate_limit_remaining: None,
            auth_username,
            theme,
            theme_colors: ThemeColors::new(theme),
            last_interaction: Instant::now(),
            visible_rows: 0,
        }
    }

    pub fn current_prs(&self) -> &[(PullRequest, ScoreResult)] {
        match self.current_view {
            View::Active => &self.active_prs,
            View::Snoozed => &self.snoozed_prs,
        }
    }

    /// All scores across active and snoozed rows: the full distribution
    /// score tiers are computed from, so colors are stable across views
    /// and don't jump when a row is snoozed.
    pub fn score_pool(&self) -> Vec<f64> {
        self.active_prs
            .iter()
            .chain(self.snoozed_prs.iter())
            .map(|(_, result)| result.score)
            .collect()
    }

    pub fn next_row(&mut self) {
        let prs = self.current_prs();
        if prs.is_empty() {
            return;
        }
        let i = match self.table_state.selected() {
            Some(i) => {
                if i >= prs.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.table_state.select(Some(i));
    }

    pub fn previous_row(&mut self) {
        let prs = self.current_prs();
        if prs.is_empty() {
            return;
        }
        let i = match self.table_state.selected() {
            Some(i) => {
                if i == 0 {
                    prs.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.table_state.select(Some(i));
    }

    /// PageDown: advance by one viewport of rows, clamped at the last row.
    pub fn page_down(&mut self) {
        let len = self.current_prs().len();
        if len == 0 {
            return;
        }
        let page = self.visible_rows.max(1);
        let i = self
            .table_state
            .selected()
            .map_or(0, |i| (i + page).min(len - 1));
        self.table_state.select(Some(i));
    }

    /// PageUp: move back by one viewport of rows, clamped at the first row.
    pub fn page_up(&mut self) {
        if self.current_prs().is_empty() {
            return;
        }
        let page = self.visible_rows.max(1);
        let i = self
            .table_state
            .selected()
            .map_or(0, |i| i.saturating_sub(page));
        self.table_state.select(Some(i));
    }

    /// g: jump to the first row.
    pub fn jump_top(&mut self) {
        if !self.current_prs().is_empty() {
            self.table_state.select(Some(0));
        }
    }

    /// G: jump to the last row.
    pub fn jump_bottom(&mut self) {
        let len = self.current_prs().len();
        if len > 0 {
            self.table_state.select(Some(len - 1));
        }
    }

    /// Rows actually on screen: viewport size capped by rows left after the
    /// scroll offset. Zero only when the list is empty.
    fn viewport(&self) -> Option<(usize, usize)> {
        let len = self.current_prs().len();
        if len == 0 {
            return None;
        }
        let offset = self.table_state.offset().min(len - 1);
        let visible = self.visible_rows.max(1).min(len - offset);
        Some((offset, visible))
    }

    /// H (vim): jump to the top row of the viewport.
    pub fn jump_high(&mut self) {
        if let Some((offset, _)) = self.viewport() {
            self.table_state.select(Some(offset));
        }
    }

    /// M (vim): jump to the middle row of the viewport.
    pub fn jump_middle(&mut self) {
        if let Some((offset, visible)) = self.viewport() {
            self.table_state.select(Some(offset + visible / 2));
        }
    }

    /// L (vim): jump to the bottom row of the viewport.
    pub fn jump_low(&mut self) {
        if let Some((offset, visible)) = self.viewport() {
            self.table_state.select(Some(offset + visible - 1));
        }
    }

    pub fn selected_pr(&self) -> Option<&PullRequest> {
        let prs = self.current_prs();
        self.table_state
            .selected()
            .and_then(|i| prs.get(i).map(|(pr, _)| pr))
    }

    pub fn push_undo(&mut self, action: UndoAction) {
        self.undo_stack.push_front(action);
        if self.undo_stack.len() > MAX_UNDO {
            self.undo_stack.pop_back();
        }
    }

    pub fn update_flash(&mut self) {
        if let Some((_, timestamp)) = self.flash_message {
            if timestamp.elapsed().as_secs() >= 3 {
                self.flash_message = None;
            }
        }
    }

    pub fn show_flash(&mut self, msg: String) {
        self.flash_message = Some((msg, Instant::now()));
    }

    pub fn auto_refresh_interval(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.config.auto_refresh_interval)
    }

    /// Open the selected PR in the browser
    pub fn open_selected(&self) -> anyhow::Result<()> {
        if let Some(pr) = self.selected_pr() {
            crate::browser::open_url(&pr.url)?;
        }
        Ok(())
    }

    /// Start snooze input mode (works on both Active and Snoozed views)
    pub fn start_snooze_input(&mut self) {
        if self.selected_pr().is_some() {
            self.input_mode = InputMode::SnoozeInput;
            self.snooze_input.clear();
        }
    }

    /// Confirm and apply the snooze input
    pub fn confirm_snooze_input(&mut self) {
        // Get selected PR info before mutating
        let (url, title) = match self.selected_pr() {
            Some(pr) => (pr.url.clone(), pr.title.clone()),
            None => {
                self.input_mode = InputMode::Normal;
                return;
            }
        };

        // Parse duration from input. A snooze always wakes, so an empty
        // duration is an error; permanent hiding is `i` (ignore).
        let input = self.snooze_input.trim().to_string();
        if input.is_empty() {
            self.show_flash("Enter a duration (e.g. 2h, 3d, 1w); press i to ignore".to_string());
            self.input_mode = InputMode::Normal;
            self.snooze_input.clear();
            return;
        }
        let computed_until = match humantime::parse_duration(&input) {
            Ok(duration) => Utc::now() + chrono::Duration::from_std(duration).unwrap_or_default(),
            Err(_) => {
                self.show_flash(format!("Invalid duration: '{}'", input));
                self.input_mode = InputMode::Normal;
                self.snooze_input.clear();
                return;
            }
        };

        // Capture the previous wake time before overwriting (needed for undo
        // on re-snooze). is_snoozed, not contains_key: a stale expired entry
        // is not an active manual snooze.
        let was_manually_snoozed = self.snooze_state.is_snoozed(&url);
        let old_until = self
            .snooze_state
            .snoozed_entries()
            .get(&url)
            .map(|entry| entry.snooze_until)
            .filter(|_| was_manually_snoozed);

        // Apply snooze
        self.snooze_state.snooze(url.clone(), computed_until);

        // Save to disk
        if !self.persist() {
            self.input_mode = InputMode::Normal;
            return;
        }

        // Branch behavior based on current view
        match self.current_view {
            View::Active => {
                // Push to undo stack
                self.push_undo(UndoAction::Snoozed {
                    url: url.clone(),
                    title: title.clone(),
                    was_suppressed: false,
                });

                // Move PR from active to snoozed
                self.relocate(&url);

                // Show flash message
                self.show_flash(format!("Snoozed: {} (z to undo)", title));
            }
            View::Snoozed => {
                if let Some(previous_until) = old_until {
                    // Push re-snooze to undo stack with previous wake time
                    self.push_undo(UndoAction::Resnooze {
                        url: url.clone(),
                        title: title.clone(),
                        previous_until,
                    });

                    // PR stays in snoozed list -- no move needed
                    self.show_flash(format!("Re-snoozed: {} (z to undo)", title));
                } else {
                    // Suppressed (awaiting author) row: this is a fresh
                    // manual snooze, which takes precedence over suppression
                    let was_suppressed = self.suppressed_urls.remove(&url);
                    self.push_undo(UndoAction::Snoozed {
                        url: url.clone(),
                        title: title.clone(),
                        was_suppressed,
                    });
                    self.show_flash(format!("Snoozed: {} (z to undo)", title));
                }
            }
        }

        // Return to normal mode
        self.input_mode = InputMode::Normal;
        self.snooze_input.clear();
    }

    /// Cancel snooze input
    pub fn cancel_snooze_input(&mut self) {
        self.input_mode = InputMode::Normal;
        self.snooze_input.clear();
    }

    /// Unsnooze the selected PR (only works in Snoozed view)
    pub fn unsnooze_selected(&mut self) {
        if !matches!(self.current_view, View::Snoozed) {
            return;
        }

        let (url, title, until) = match self.selected_pr() {
            Some(pr) => {
                let url = pr.url.clone();
                let title = pr.title.clone();
                // Look up snooze entry to get the until time for undo
                let until = self
                    .snooze_state
                    .snoozed_entries()
                    .get(&url)
                    .map(|entry| entry.snooze_until);
                (url, title, until)
            }
            None => return,
        };

        // Suppressed rows aren't manually snoozed (an expired entry doesn't
        // count); there is nothing to undo. They resurface on author updates,
        // mentions, or re-requests.
        if self.suppressed_urls.contains(&url) && !self.snooze_state.is_snoozed(&url) {
            self.show_flash("Awaiting author since your review; updates resurface it".to_string());
            return;
        }

        // Unsnooze
        self.snooze_state.unsnooze(&url);

        // Save to disk
        if !self.persist() {
            return;
        }

        // With the manual snooze gone, the row is governed by the policy
        // again: if its current signals still resolve to awaiting-author it
        // stays in the Snoozed view as suppressed rather than jumping to
        // Active until the next refresh.
        let became_suppressed = self.still_suppressed_now(&url).unwrap_or(false);

        // Push to undo stack
        self.push_undo(UndoAction::Unsnoozed {
            url: url.clone(),
            title: title.clone(),
            until,
            became_suppressed,
        });

        if became_suppressed {
            self.suppressed_urls.insert(url.clone());
        }
        self.relocate(&url);

        if became_suppressed {
            self.show_flash(format!("Unsnoozed: {} (awaiting author; z to undo)", title));
        } else {
            self.show_flash(format!("Unsnoozed: {} (z to undo)", title));
        }
    }

    /// Whether a snoozed row would still be suppressed (awaiting author)
    /// given its current signals and the active policy. Refreshes update
    /// signals while undo entries survive them, so callers re-evaluate
    /// rather than trusting snapshots. `None` when the policy or row cannot
    /// be resolved; callers fall back to their snapshot (or activate).
    fn still_suppressed_now(&self, url: &str) -> Option<bool> {
        let policy = crate::snooze::suppress_policy(self.config.suppress.as_ref())
            .ok()
            .flatten()?;
        let (pr, _) = self.snoozed_prs.iter().find(|(pr, _)| pr.url == url)?;
        Some(crate::snooze::is_suppressed_by_policy(
            &pr.signals,
            &policy,
            Utc::now(),
        ))
    }

    /// Undo the last snooze or unsnooze action
    pub fn undo_last(&mut self) {
        let action = match self.undo_stack.pop_front() {
            Some(action) => action,
            None => {
                self.show_flash("Nothing to undo".to_string());
                return;
            }
        };

        match action {
            UndoAction::Snoozed {
                url,
                title,
                was_suppressed,
            } => {
                // Undo a snooze: unsnooze the PR
                self.snooze_state.unsnooze(&url);

                // Save to disk
                if !self.persist() {
                    return;
                }

                // Re-evaluate against current signals for every removed
                // snooze; the snapshot only stands when policy or row can't
                // be resolved. A row can become awaiting-author (or wake)
                // while snoozed.
                let suppressed = self.still_suppressed_now(&url).unwrap_or(was_suppressed);
                if suppressed {
                    self.suppressed_urls.insert(url.clone());
                }
                self.relocate(&url);

                if suppressed {
                    self.show_flash(format!("Undid snooze: {} (awaiting author)", title));
                } else {
                    self.show_flash(format!("Undid snooze: {}", title));
                }
            }
            UndoAction::Unsnoozed {
                url,
                title,
                until,
                became_suppressed,
            } => {
                // Undo an unsnooze: restore the removed snooze, if there was one
                if let Some(until) = until {
                    self.snooze_state.snooze(url.clone(), until);
                }

                // Save to disk
                if !self.persist() {
                    return;
                }

                if became_suppressed {
                    // The row never left the Snoozed view; drop the marker so
                    // it shows as manually snoozed again.
                    self.suppressed_urls.remove(&url);
                }
                // Move PR back from active to snoozed (no-op if it never left)
                self.relocate(&url);

                self.show_flash(format!("Undid unsnooze: {}", title));
            }
            UndoAction::Resnooze {
                url,
                title,
                previous_until,
            } => {
                // Undo a re-snooze: restore the previous snooze duration
                self.snooze_state.snooze(url.clone(), previous_until);

                // Save to disk
                if !self.persist() {
                    return;
                }

                // PR stays in snoozed list -- no move needed
                self.show_flash(format!("Undid re-snooze: {}", title));
            }
        }
    }

    /// Write both hide files. On failure, flash the error and return false
    /// so the caller can abort before touching the in-memory lists.
    fn persist(&mut self) -> bool {
        match crate::hide::save_hide_state(&self.hide_paths, &self.snooze_state, &self.ignore_state)
        {
            Ok(()) => true,
            Err(e) => {
                self.show_flash(format!("Failed to save hide state: {}", e));
                false
            }
        }
    }

    /// The view a row belongs in, derived from its hide state: a manual
    /// snooze or a suppression marker puts it in Snoozed, otherwise Active.
    fn target_view(&self, url: &str) -> View {
        if self.snooze_state.is_snoozed(url) || self.suppressed_urls.contains(url) {
            View::Snoozed
        } else {
            View::Active
        }
    }

    fn list_mut(&mut self, view: View) -> &mut Vec<(PullRequest, ScoreResult)> {
        match view {
            View::Active => &mut self.active_prs,
            View::Snoozed => &mut self.snoozed_prs,
        }
    }

    /// Move the row for `url` into whichever list its current state says it
    /// belongs in. Callers mutate snooze state and suppression markers, then
    /// relocate; a row already in the right list stays put.
    fn relocate(&mut self, url: &str) {
        let target = self.target_view(url);
        let source = if self.active_prs.iter().any(|(pr, _)| pr.url == url) {
            View::Active
        } else if self.snoozed_prs.iter().any(|(pr, _)| pr.url == url) {
            View::Snoozed
        } else {
            return;
        };
        if source == target {
            return;
        }

        let source_list = self.list_mut(source);
        let pos = source_list
            .iter()
            .position(|(pr, _)| pr.url == url)
            .expect("row found in source list above");
        let pr_entry = source_list.remove(pos);

        // Insert into destination list, maintaining score-descending sort
        let dest_list = self.list_mut(target);
        let insert_pos = dest_list
            .iter()
            .position(|(_, score)| score.score < pr_entry.1.score)
            .unwrap_or(dest_list.len());
        dest_list.insert(insert_pos, pr_entry);

        // Fix table selection to stay valid
        let current_list = self.current_prs();
        if current_list.is_empty() {
            self.table_state.select(None);
        } else if let Some(selected) = self.table_state.selected() {
            if selected >= current_list.len() {
                self.table_state.select(Some(current_list.len() - 1));
            }
        }
    }

    /// Toggle between Active and Snoozed views
    pub fn toggle_view(&mut self) {
        self.current_view = match self.current_view {
            View::Active => View::Snoozed,
            View::Snoozed => View::Active,
        };

        // Reset selection to first item in the new view, or None if empty
        let prs = self.current_prs();
        if prs.is_empty() {
            self.table_state.select(None);
        } else {
            self.table_state.select(Some(0));
        }
    }

    /// Show help overlay
    pub fn show_help(&mut self) {
        self.input_mode = InputMode::Help;
    }

    /// Dismiss help overlay
    pub fn dismiss_help(&mut self) {
        self.input_mode = InputMode::Normal;
    }

    /// Show score breakdown overlay
    pub fn show_score_breakdown(&mut self) {
        if self.selected_pr().is_some() {
            self.input_mode = InputMode::ScoreBreakdown;
        }
    }

    /// Dismiss score breakdown overlay
    pub fn dismiss_score_breakdown(&mut self) {
        self.input_mode = InputMode::Normal;
    }

    /// Get the selected PR's ScoreResult
    pub fn selected_score_result(&self) -> Option<&crate::scoring::ScoreResult> {
        let prs = self.current_prs();
        self.table_state
            .selected()
            .and_then(|i| prs.get(i).map(|(_, sr)| sr))
    }

    /// Update PRs with fresh data from fetch
    pub fn update_prs(&mut self, fetched: crate::fetch::FetchedPrs) {
        let crate::fetch::FetchedPrs {
            active,
            suppressed,
            snoozed,
            ignored,
            rate_limit_remaining,
        } = fetched;

        // Suppressed PRs share the Snoozed view, tagged "awaiting author"
        let (snoozed_merged, suppressed_urls) = merge_snoozed_lists(snoozed, suppressed);

        // Wake-reason tags derived from the same effective policy that
        // partitioning uses. Snoozed rows are included: a row that wakes
        // while manually snoozed keeps its tag when it later moves to
        // Active in-memory (unsnooze/undo) before the next refresh.
        let policy = crate::snooze::suppress_policy(self.config.suppress.as_ref())
            .ok()
            .flatten();
        let now = Utc::now();
        let mut review_states = compute_review_states(&active, policy.as_ref(), now);
        review_states.extend(compute_review_states(&snoozed_merged, policy.as_ref(), now));
        self.review_states = review_states;

        // Replace PR lists
        self.active_prs = active;
        self.snoozed_prs = snoozed_merged;
        self.suppressed_urls = suppressed_urls;
        self.ignored_prs = ignored;

        // Update rate limit info
        self.rate_limit_remaining = rate_limit_remaining;

        // Preserve selection if possible
        let current_list = self.current_prs();
        if current_list.is_empty() {
            self.table_state.select(None);
        } else if let Some(selected) = self.table_state.selected() {
            // Clamp to new list length
            if selected >= current_list.len() {
                self.table_state.select(Some(current_list.len() - 1));
            }
        } else {
            // No selection before, select first if list is non-empty
            self.table_state.select(Some(0));
        }

        // Reload hide state from disk (in case it was modified externally)
        if let Ok((snooze, ignore)) = crate::hide::load_hide_state(&self.hide_paths) {
            self.snooze_state = snooze;
            self.ignore_state = ignore;
        }

        // Update refresh timestamp
        self.last_refresh = Instant::now();

        // Show flash message
        let active_count = self.active_prs.len();
        let awaiting_count = self.suppressed_urls.len();
        let snoozed_count = self.snoozed_prs.len() - awaiting_count;
        if awaiting_count > 0 {
            self.show_flash(format!(
                "Refreshed ({} active, {} awaiting author, {} snoozed)",
                active_count, awaiting_count, snoozed_count
            ));
        } else {
            self.show_flash(format!(
                "Refreshed ({} active, {} snoozed)",
                active_count, snoozed_count
            ));
        }
    }

    /// Advance the loading spinner animation frame
    pub fn advance_spinner(&mut self) {
        self.spinner_frame = self.spinner_frame.wrapping_add(1);
    }
}

// LOCKED: regression for since-my-review review workflow (feat/since-my-review).
// All tests in this module are locked. Snoozed-view merge and wake-state computation for row tags.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::review_state::{ReviewSignals, ReviewState};
    use chrono::{Duration, TimeZone};

    fn ts(s: &str) -> DateTime<Utc> {
        s.parse().unwrap()
    }

    fn test_pr(url: &str, signals: ReviewSignals) -> PullRequest {
        PullRequest {
            title: format!("PR {}", url),
            number: 1,
            author: "author".to_string(),
            repo: "o/r".to_string(),
            url: url.to_string(),
            created_at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
            updated_at: Utc.with_ymd_and_hms(2026, 1, 2, 0, 0, 0).unwrap(),
            additions: 1,
            deletions: 1,
            approvals: 0,
            draft: false,
            labels: vec![],
            user_has_reviewed: false,
            filtered_size: None,
            signals,
        }
    }

    fn scored(url: &str, score: f64) -> (PullRequest, ScoreResult) {
        (
            test_pr(url, ReviewSignals::default()),
            ScoreResult {
                score,
                ..Default::default()
            },
        )
    }

    fn test_app(name: &str) -> App {
        App::new(
            Vec::new(),
            Vec::new(),
            SnoozeState::new(),
            IgnoreState::new(),
            HidePaths::in_dir(&std::env::temp_dir().join(format!(
                "pr-pal-app-test-{}-{}",
                std::process::id(),
                name
            ))),
            Config {
                scoring: None,
                queries: vec![],
                auto_refresh_interval: 300,
                theme: "dark".to_string(),
                suppress: None,
            },
            CacheConfig { enabled: false },
            None,
            false,
            Some("me".to_string()),
            Theme::Dark,
        )
    }

    /// Seed an app with one suppressed (awaiting author) row in the Snoozed
    /// view, selected.
    fn app_with_suppressed_row(name: &str, url: &str) -> App {
        let mut app = test_app(name);
        app.snoozed_prs = vec![scored(url, 1.0)];
        app.suppressed_urls.insert(url.to_string());
        app.current_view = View::Snoozed;
        app.table_state.select(Some(0));
        app
    }

    /// Seed an app with `n` active rows, first row selected.
    fn app_with_rows(name: &str, n: usize) -> App {
        let mut app = test_app(name);
        app.active_prs = (0..n)
            .map(|i| scored(&format!("https://x/{i}"), 100.0 - i as f64))
            .collect();
        app.table_state.select(Some(0));
        app
    }

    #[test]
    fn page_keys_move_by_viewport_and_clamp() {
        let mut app = app_with_rows("page-keys", 30);
        app.visible_rows = 10;
        app.page_down();
        assert_eq!(app.table_state.selected(), Some(10));
        app.page_down();
        app.page_down();
        // Clamped at the last row, no wraparound.
        assert_eq!(app.table_state.selected(), Some(29));
        app.page_up();
        assert_eq!(app.table_state.selected(), Some(19));
        app.page_up();
        app.page_up();
        assert_eq!(app.table_state.selected(), Some(0));
    }

    #[test]
    fn page_keys_fall_back_to_single_row_before_first_render() {
        let mut app = app_with_rows("page-fallback", 5);
        app.visible_rows = 0;
        app.page_down();
        assert_eq!(app.table_state.selected(), Some(1));
    }

    #[test]
    fn vim_jumps_target_top_middle_bottom_of_viewport() {
        let mut app = app_with_rows("vim-jumps", 30);
        app.visible_rows = 10;
        *app.table_state.offset_mut() = 5; // rows 5..15 visible
        app.jump_high();
        assert_eq!(app.table_state.selected(), Some(5));
        app.jump_middle();
        assert_eq!(app.table_state.selected(), Some(10));
        app.jump_low();
        assert_eq!(app.table_state.selected(), Some(14));
    }

    #[test]
    fn vim_jumps_clamp_when_viewport_outruns_list() {
        let mut app = app_with_rows("vim-clamp", 8);
        app.visible_rows = 10;
        app.jump_low();
        assert_eq!(app.table_state.selected(), Some(7));
        app.jump_middle();
        assert_eq!(app.table_state.selected(), Some(4));
    }

    #[test]
    fn top_and_bottom_jumps() {
        let mut app = app_with_rows("top-bottom", 12);
        app.jump_bottom();
        assert_eq!(app.table_state.selected(), Some(11));
        app.jump_top();
        assert_eq!(app.table_state.selected(), Some(0));
    }

    #[test]
    fn nav_keys_are_safe_on_empty_lists() {
        let mut app = test_app("nav-empty");
        app.page_down();
        app.page_up();
        app.jump_top();
        app.jump_bottom();
        app.jump_high();
        app.jump_middle();
        app.jump_low();
        assert_eq!(app.table_state.selected(), None);
    }

    #[test]
    fn score_pool_includes_active_and_snoozed() {
        let mut app = test_app("score-pool");
        app.active_prs = vec![scored("https://x/1", 100.0)];
        app.snoozed_prs = vec![scored("https://x/2", 500.0), scored("https://x/3", 20.0)];
        let mut pool = app.score_pool();
        pool.sort_by(|a, b| a.partial_cmp(b).unwrap());
        assert_eq!(pool, vec![20.0, 100.0, 500.0]);
    }

    // LOCKED: regression for lost wake tags on snoozed rows (pr-pal#2 Copilot review).
    // A manually snoozed PR can wake during a refresh; when it later moves to
    // Active in-memory (unsnooze/undo) its wake tag must still render, so
    // update_prs must compute review states for Snoozed rows too.
    #[test]
    fn update_prs_computes_wake_states_for_snoozed_rows() {
        let url = "https://x/snoozed-woken";
        let mut app = test_app("wake-states-snoozed");
        app.config.suppress = Some(crate::config::SuppressConfig {
            awaiting_author: true,
            wake_on: vec![crate::config::WakeEvent::Push],
            resurface_after: "21d".to_string(),
        });

        // Manually snoozed row whose author pushed after my review.
        let mut signals = ReviewSignals {
            my_last_review_at: Some(Utc::now() - Duration::days(5)),
            ..Default::default()
        };
        signals.last_commit_at = Some(Utc::now() - Duration::days(1));
        let snoozed = vec![(test_pr(url, signals), ScoreResult::default())];

        app.update_prs(crate::fetch::FetchedPrs {
            active: vec![],
            suppressed: vec![],
            snoozed,
            ignored: vec![],
            rate_limit_remaining: None,
        });

        assert_eq!(
            app.review_states.get(url),
            Some(&ReviewState::Pushed),
            "snoozed rows must carry their wake state"
        );
    }

    // LOCKED: regression for expired-snooze guards (pr-pal#2 Copilot review).
    // An expired manual snooze entry must not let `u` activate a row the
    // partition still classifies as suppressed (awaiting author).
    #[test]
    fn unsnooze_guard_holds_when_stale_expired_entry_exists() {
        let url = "https://x/suppressed";
        let mut app = app_with_suppressed_row("unsnooze-guard", url);
        // Stale entry: expired an hour ago, not yet cleaned up.
        app.snooze_state
            .snooze(url.to_string(), Utc::now() - Duration::hours(1));

        app.unsnooze_selected();

        assert_eq!(app.snoozed_prs.len(), 1, "row must stay in Snoozed view");
        assert!(
            app.active_prs.is_empty(),
            "suppressed row must not activate"
        );
        assert!(app.undo_stack.is_empty());
    }

    // LOCKED: regression for expired-snooze guards (pr-pal#2 Copilot review).
    // Snoozing a suppressed row that has a stale expired entry is a fresh
    // manual snooze: the suppression marker must clear, not the re-snooze path.
    #[test]
    fn snoozing_suppressed_row_with_stale_entry_clears_marker() {
        let url = "https://x/suppressed";
        let mut app = app_with_suppressed_row("stale-resnooze", url);
        app.snooze_state
            .snooze(url.to_string(), Utc::now() - Duration::hours(1));
        app.input_mode = InputMode::SnoozeInput;
        app.snooze_input = "1d".to_string();

        app.confirm_snooze_input();

        assert!(
            !app.suppressed_urls.contains(url),
            "marker must clear on fresh snooze"
        );
        assert!(matches!(
            app.undo_stack.front(),
            Some(UndoAction::Snoozed { .. })
        ));
    }

    // LOCKED: regression for undo of suppressed-row snooze (pr-pal#2 Copilot review).
    // Undoing a manual snooze of a suppressed row must restore suppression,
    // not activate the row: no wake event occurred.
    #[test]
    fn undo_snooze_of_suppressed_row_restores_suppression() {
        let url = "https://x/suppressed";
        let mut app = app_with_suppressed_row("undo-suppressed", url);
        app.input_mode = InputMode::SnoozeInput;
        app.snooze_input = "1d".to_string();
        app.confirm_snooze_input();
        assert!(!app.suppressed_urls.contains(url), "precondition");

        app.undo_last();

        assert!(
            !app.snooze_state.is_snoozed(url),
            "manual snooze must be undone"
        );
        assert!(
            app.suppressed_urls.contains(url),
            "suppression must be restored"
        );
        assert_eq!(app.snoozed_prs.len(), 1, "row must stay in Snoozed view");
        assert!(app.active_prs.is_empty());
    }

    // LOCKED: regression for unsnooze bypassing suppression (pr-pal#2 Copilot review).
    // Removing a manual snooze must re-evaluate the row: if its current
    // signals still resolve to awaiting-author, it stays in the Snoozed view
    // as suppressed instead of jumping to Active until the next refresh.
    #[test]
    fn unsnooze_keeps_awaiting_author_row_suppressed() {
        let url = "https://x/manual-snoozed";
        let mut app = test_app("unsnooze-reeval");
        app.config.suppress = Some(crate::config::SuppressConfig {
            awaiting_author: true,
            wake_on: vec![crate::config::WakeEvent::Push],
            resurface_after: "21d".to_string(),
        });
        app.snoozed_prs = vec![scored(url, 1.0)];
        // Reviewed 5 days ago, nothing since: policy says awaiting author.
        app.snoozed_prs[0].0.signals.my_last_review_at = Some(Utc::now() - Duration::days(5));
        app.snooze_state
            .snooze(url.to_string(), Utc::now() + Duration::days(365));
        app.current_view = View::Snoozed;
        app.table_state.select(Some(0));

        app.unsnooze_selected();

        assert!(!app.snooze_state.is_snoozed(url), "manual snooze removed");
        assert!(
            app.active_prs.is_empty(),
            "awaiting-author row must not activate"
        );
        assert_eq!(app.snoozed_prs.len(), 1, "row stays in the Snoozed view");
        assert!(
            app.suppressed_urls.contains(url),
            "suppression marker must be added"
        );
    }

    // LOCKED: regression for unsnooze bypassing suppression (pr-pal#2 Copilot review).
    // Undoing that unsnooze restores the manual snooze without duplicating
    // the row or leaving the suppression marker behind.
    #[test]
    fn undo_unsnooze_of_awaiting_author_row_restores_manual_snooze() {
        let url = "https://x/manual-snoozed-undo";
        let mut app = test_app("unsnooze-reeval-undo");
        app.config.suppress = Some(crate::config::SuppressConfig {
            awaiting_author: true,
            wake_on: vec![crate::config::WakeEvent::Push],
            resurface_after: "21d".to_string(),
        });
        app.snoozed_prs = vec![scored(url, 1.0)];
        app.snoozed_prs[0].0.signals.my_last_review_at = Some(Utc::now() - Duration::days(5));
        app.snooze_state
            .snooze(url.to_string(), Utc::now() + Duration::days(365));
        app.current_view = View::Snoozed;
        app.table_state.select(Some(0));

        app.unsnooze_selected();
        app.undo_last();

        assert!(app.snooze_state.is_snoozed(url), "manual snooze restored");
        assert!(
            !app.suppressed_urls.contains(url),
            "suppression marker must be removed; the manual snooze wins again"
        );
        assert_eq!(app.snoozed_prs.len(), 1, "row must not duplicate");
        assert!(app.active_prs.is_empty());
    }

    // LOCKED: regression for undo trusting a stale non-suppressed snapshot (pr-pal#2 Copilot review).
    // A row snoozed from Active can become awaiting-author during a refresh
    // (e.g. my review arrived). Undoing the snooze must re-evaluate current
    // signals for every removed snooze, not only ones snoozed while
    // suppressed.
    #[test]
    fn undo_snooze_reevaluates_rows_snoozed_from_active() {
        let url = "https://x/active-row";
        let mut app = test_app("undo-reeval-active");
        app.config.suppress = Some(crate::config::SuppressConfig {
            awaiting_author: true,
            wake_on: vec![crate::config::WakeEvent::Push],
            resurface_after: "21d".to_string(),
        });
        app.active_prs = vec![scored(url, 1.0)];
        app.current_view = View::Active;
        app.table_state.select(Some(0));

        app.input_mode = InputMode::SnoozeInput;
        app.snooze_input = "1d".to_string();
        app.confirm_snooze_input();

        // A refresh delivered new signals: I reviewed it, nothing since.
        app.snoozed_prs[0].0.signals.my_last_review_at = Some(Utc::now() - Duration::days(1));

        app.undo_last();

        assert!(!app.snooze_state.is_snoozed(url));
        assert!(
            app.active_prs.is_empty(),
            "awaiting-author row must not activate"
        );
        assert_eq!(app.snoozed_prs.len(), 1, "row stays in the Snoozed view");
        assert!(
            app.suppressed_urls.contains(url),
            "suppression marker must be added"
        );
    }

    // LOCKED: regression for stale undo suppression snapshot (pr-pal#2 Copilot review).
    // Undo must re-evaluate the row's current signals: if a wake event
    // arrived (via refresh) while the row was manually snoozed, undoing the
    // snooze activates it instead of restoring a stale awaiting-author state.
    #[test]
    fn undo_snooze_reevaluates_signals_and_activates_woken_row() {
        let url = "https://x/suppressed";
        let mut app = app_with_suppressed_row("undo-reeval", url);
        app.config.suppress = Some(crate::config::SuppressConfig {
            awaiting_author: true,
            wake_on: vec![
                crate::config::WakeEvent::Push,
                crate::config::WakeEvent::Mention,
                crate::config::WakeEvent::ReviewRequest,
            ],
            resurface_after: "21d".to_string(),
        });
        // Reviewed 5 days ago, nothing since: genuinely suppressed.
        app.snoozed_prs[0].0.signals.my_last_review_at = Some(Utc::now() - Duration::days(5));

        app.input_mode = InputMode::SnoozeInput;
        app.snooze_input = "1d".to_string();
        app.confirm_snooze_input();

        // A refresh delivered new signals while snoozed: the author pushed.
        app.snoozed_prs[0].0.signals.last_commit_at = Some(Utc::now() - Duration::hours(1));

        app.undo_last();

        assert!(!app.snooze_state.is_snoozed(url));
        assert!(
            !app.suppressed_urls.contains(url),
            "woken row must not be re-suppressed"
        );
        assert_eq!(app.active_prs.len(), 1, "woken row must activate");
        assert!(app.snoozed_prs.is_empty());
    }

    #[test]
    fn merge_snoozed_lists_sorts_by_score_and_tracks_suppressed() {
        let snoozed = vec![scored("https://x/1", 50.0)];
        let suppressed = vec![scored("https://x/2", 100.0), scored("https://x/3", 10.0)];

        let (merged, suppressed_urls) = merge_snoozed_lists(snoozed, suppressed);

        let urls: Vec<&str> = merged.iter().map(|(pr, _)| pr.url.as_str()).collect();
        assert_eq!(urls, vec!["https://x/2", "https://x/1", "https://x/3"]);
        assert!(suppressed_urls.contains("https://x/2"));
        assert!(suppressed_urls.contains("https://x/3"));
        assert!(!suppressed_urls.contains("https://x/1"));
    }

    #[test]
    fn merge_snoozed_lists_breaks_score_ties_by_age() {
        let mut older = scored("https://x/old", 50.0);
        older.0.created_at = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
        let newer = scored("https://x/new", 50.0);

        let (merged, _) = merge_snoozed_lists(vec![newer], vec![older]);

        let urls: Vec<&str> = merged.iter().map(|(pr, _)| pr.url.as_str()).collect();
        assert_eq!(urls, vec!["https://x/old", "https://x/new"]);
    }

    #[test]
    fn compute_review_states_maps_urls_to_states() {
        let now = ts("2026-09-10T00:00:00Z");
        let pushed = (
            test_pr(
                "https://x/pushed",
                ReviewSignals {
                    my_last_review_at: Some(ts("2026-09-01T00:00:00Z")),
                    last_commit_at: Some(ts("2026-09-02T00:00:00Z")),
                    ..Default::default()
                },
            ),
            ScoreResult::default(),
        );
        let not_reviewed = scored("https://x/plain", 1.0);

        let states = compute_review_states(&[pushed, not_reviewed], None, now);

        assert_eq!(states.get("https://x/pushed"), Some(&ReviewState::Pushed));
        assert_eq!(
            states.get("https://x/plain"),
            Some(&ReviewState::NotReviewed)
        );
    }

    #[test]
    fn compute_review_states_applies_valve() {
        let now = ts("2026-09-30T00:00:00Z");
        let stalled = (
            test_pr(
                "https://x/stalled",
                ReviewSignals {
                    my_last_review_at: Some(ts("2026-09-01T00:00:00Z")),
                    ..Default::default()
                },
            ),
            ScoreResult::default(),
        );

        let policy = crate::snooze::SuppressPolicy {
            wake_on: vec![],
            resurface_after: Some(Duration::days(21)),
        };
        let states = compute_review_states(&[stalled], Some(&policy), now);

        assert_eq!(states.get("https://x/stalled"), Some(&ReviewState::Stalled));
    }

    // LOCKED: regression for policy-aware wake tags (pr-pal#2 Copilot review).
    // Tags must reflect the effective policy: with wake_on [mention], a PR
    // that was pushed then mentioned tags as (mentioned), not (updated).
    #[test]
    fn compute_review_states_respects_wake_policy() {
        let now = ts("2026-09-10T00:00:00Z");
        let pr = (
            test_pr(
                "https://x/mixed",
                ReviewSignals {
                    my_last_review_at: Some(ts("2026-09-01T00:00:00Z")),
                    last_commit_at: Some(ts("2026-09-02T00:00:00Z")),
                    mentioned_at: Some(ts("2026-09-03T00:00:00Z")),
                    ..Default::default()
                },
            ),
            ScoreResult::default(),
        );
        let policy = crate::snooze::SuppressPolicy {
            wake_on: vec![crate::config::WakeEvent::Mention],
            resurface_after: Some(Duration::days(21)),
        };

        let states = compute_review_states(&[pr], Some(&policy), now);

        assert_eq!(states.get("https://x/mixed"), Some(&ReviewState::Mentioned));
    }
}

/// Tests for the ignore feature and the snooze changes it brought (no
/// indefinite snooze). Kept separate from the locked module above.
#[cfg(test)]
mod ignore_tests {
    use super::*;
    use crate::review_state::ReviewSignals;
    use chrono::TimeZone;

    fn test_pr(url: &str) -> PullRequest {
        PullRequest {
            title: format!("PR {}", url),
            number: 1,
            author: "author".to_string(),
            repo: "o/r".to_string(),
            url: url.to_string(),
            created_at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
            updated_at: Utc.with_ymd_and_hms(2026, 1, 2, 0, 0, 0).unwrap(),
            additions: 1,
            deletions: 1,
            approvals: 0,
            draft: false,
            labels: vec![],
            user_has_reviewed: false,
            filtered_size: None,
            signals: ReviewSignals::default(),
        }
    }

    fn scored(url: &str, score: f64) -> (PullRequest, ScoreResult) {
        (
            test_pr(url),
            ScoreResult {
                score,
                ..Default::default()
            },
        )
    }

    fn test_app(name: &str) -> App {
        let dir = std::env::temp_dir().join(format!(
            "pr-pal-ignore-test-{}-{}",
            std::process::id(),
            name
        ));
        App::new(
            Vec::new(),
            Vec::new(),
            SnoozeState::new(),
            IgnoreState::new(),
            HidePaths::in_dir(&dir),
            Config {
                scoring: None,
                queries: vec![],
                auto_refresh_interval: 300,
                theme: "dark".to_string(),
                suppress: None,
            },
            CacheConfig { enabled: false },
            None,
            false,
            Some("me".to_string()),
            Theme::Dark,
        )
    }

    /// One active row, selected.
    fn app_with_active_row(name: &str, url: &str) -> App {
        let mut app = test_app(name);
        app.active_prs = vec![scored(url, 1.0)];
        app.table_state.select(Some(0));
        app
    }

    // Snooze means "wake me later": an empty duration is an error, not an
    // open-ended snooze. Permanent hiding is `i` (ignore).
    #[test]
    fn empty_snooze_input_is_rejected() {
        let url = "https://x/active";
        let mut app = app_with_active_row("empty-snooze", url);
        app.input_mode = InputMode::SnoozeInput;
        app.snooze_input = "   ".to_string();

        app.confirm_snooze_input();

        assert!(!app.snooze_state.is_snoozed(url), "nothing snoozed");
        assert_eq!(app.active_prs.len(), 1, "row stays Active");
        assert!(app.undo_stack.is_empty());
        assert_eq!(app.input_mode, InputMode::Normal);
        let flash = app.flash_message.as_ref().map(|(m, _)| m.as_str());
        assert!(
            flash.is_some_and(|m| m.contains("duration")),
            "flash should ask for a duration, got {flash:?}"
        );
    }
}
