//! The two ways a PR can be hidden, and how they combine.
//!
//! A snooze is timed and always wakes; an ignore is permanent. Ignore is
//! the stronger hide, so it is applied first when partitioning and wins if
//! a URL is somehow in both states.
//!
//! Each lives in its own file in the config dir. `snooze.json` keeps its
//! pr-bro / pr-pal 1.x schema so older readers still load it; the one
//! legacy shape, `snooze_until: null` (an indefinite snooze), is read as an
//! ignore because that was always its intent. Saving writes both files
//! together so they cannot drift apart.

use crate::github::types::PullRequest;
use crate::ignore::{IgnoreEntry, IgnoreState};
use crate::snooze::{partition_prs, SnoozeState, SuppressPolicy};
use anyhow::Result;
use chrono::{DateTime, Utc};
use std::path::{Path, PathBuf};

/// Fetched PRs split by hide state, unscored.
#[derive(Debug)]
pub struct HiddenPrs {
    pub active: Vec<PullRequest>,
    /// Awaiting the author under the suppression policy.
    pub suppressed: Vec<PullRequest>,
    pub snoozed: Vec<PullRequest>,
    /// Oldest ignore first. Ignored PRs are a record, not a queue, so they
    /// are never ranked by score.
    pub ignored: Vec<PullRequest>,
}

/// Split PRs into active / suppressed / snoozed / ignored.
///
/// Ignore is checked first so an ignored PR never also shows as snoozed or
/// suppressed; the rest go through `partition_prs`, where a manual snooze
/// outranks suppression.
pub fn partition_hidden(
    prs: Vec<PullRequest>,
    snooze: &SnoozeState,
    ignore: &IgnoreState,
    policy: Option<&SuppressPolicy>,
    now: DateTime<Utc>,
) -> HiddenPrs {
    let (mut ignored, rest): (Vec<PullRequest>, Vec<PullRequest>) =
        prs.into_iter().partition(|pr| ignore.is_ignored(&pr.url));
    ignored.sort_by_key(|pr| ignore.ignored[&pr.url].ignored_at);

    let rest = partition_prs(rest, snooze, policy, now);
    HiddenPrs {
        active: rest.active,
        suppressed: rest.suppressed,
        snoozed: rest.snoozed,
        ignored,
    }
}

#[derive(Debug, Clone)]
pub struct HidePaths {
    pub snooze: PathBuf,
    pub ignore: PathBuf,
}

impl HidePaths {
    /// `~/.config/pr-pal/snooze.json` and `~/.config/pr-pal/ignore.json`
    pub fn default_paths() -> Self {
        Self::in_dir(&crate::config::get_config_dir())
    }

    pub fn in_dir(dir: &Path) -> Self {
        Self {
            snooze: dir.join("snooze.json"),
            ignore: dir.join("ignore.json"),
        }
    }
}

/// Load both files. Missing files are empty states. Legacy indefinite
/// snoozes become ignores (ignored when they were snoozed); a URL present
/// in both files resolves to ignored, the stronger hide.
pub fn load_hide_state(paths: &HidePaths) -> Result<(SnoozeState, IgnoreState)> {
    let loaded = crate::snooze::load_snooze_file(&paths.snooze)?;
    let mut snooze = loaded.state;
    let mut ignore = crate::ignore::load_ignore_state(&paths.ignore)?;

    for (url, snoozed_at) in loaded.legacy_indefinite {
        // Keep the newer ignore.json timestamp if the URL is already there.
        ignore.ignored.entry(url).or_insert(IgnoreEntry {
            ignored_at: snoozed_at,
        });
    }
    snooze.snoozed.retain(|url, _| !ignore.is_ignored(url));

    Ok((snooze, ignore))
}

/// Save both files. The ignore file goes first so a crash between the two
/// writes leaves the stronger state on disk.
pub fn save_hide_state(
    paths: &HidePaths,
    snooze: &SnoozeState,
    ignore: &IgnoreState,
) -> Result<()> {
    crate::ignore::save_ignore_state(&paths.ignore, ignore)?;
    crate::snooze::save_snooze_state(&paths.snooze, snooze)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::WakeEvent;
    use chrono::Duration;
    use std::fs;

    const URL_A: &str = "https://github.com/o/r/pull/1";
    const URL_B: &str = "https://github.com/o/r/pull/2";

    fn temp_paths(name: &str) -> HidePaths {
        let dir =
            std::env::temp_dir().join(format!("pr-pal-hide-test-{}-{}", std::process::id(), name));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        HidePaths::in_dir(&dir)
    }

    fn ts(s: &str) -> DateTime<Utc> {
        s.parse().unwrap()
    }

    /// A pr-bro / pr-pal 1.x snooze.json with one indefinite and one timed entry.
    fn legacy_snooze_json() -> String {
        format!(
            r#"{{
  "version": 1,
  "snoozed": {{
    "{URL_A}": {{ "snoozed_at": "2026-03-01T10:00:00Z", "snooze_until": null }},
    "{URL_B}": {{ "snoozed_at": "2026-03-02T10:00:00Z", "snooze_until": "2999-01-01T00:00:00Z" }}
  }}
}}"#
        )
    }

    #[test]
    fn missing_files_load_as_empty_states() {
        let paths = temp_paths("missing");
        let (snooze, ignore) = load_hide_state(&paths).unwrap();
        assert!(snooze.snoozed.is_empty());
        assert!(ignore.ignored.is_empty());
    }

    // LOCKED: regression for legacy indefinite snoozes (feat/ignore).
    // `snooze_until: null` from pr-bro or pr-pal 1.x must load as an ignore,
    // stamped with its original snooze time, and must not stay a snooze.
    #[test]
    fn legacy_indefinite_snooze_loads_as_ignore() {
        let paths = temp_paths("legacy-null");
        fs::write(&paths.snooze, legacy_snooze_json()).unwrap();

        let (snooze, ignore) = load_hide_state(&paths).unwrap();

        assert!(ignore.is_ignored(URL_A), "null until means ignored");
        assert_eq!(
            ignore.ignored_entries()[URL_A].ignored_at,
            ts("2026-03-01T10:00:00Z"),
            "ignored_at inherits snoozed_at"
        );
        assert!(!snooze.snoozed.contains_key(URL_A));
    }

    #[test]
    fn timed_snooze_loads_as_snooze() {
        let paths = temp_paths("legacy-timed");
        fs::write(&paths.snooze, legacy_snooze_json()).unwrap();

        let (snooze, ignore) = load_hide_state(&paths).unwrap();

        assert!(snooze.is_snoozed(URL_B));
        assert_eq!(
            snooze.snoozed[URL_B].snooze_until,
            ts("2999-01-01T00:00:00Z")
        );
        assert!(!ignore.is_ignored(URL_B));
    }

    #[test]
    fn ignore_file_merges_with_converted_legacy_entries() {
        let paths = temp_paths("merge");
        fs::write(&paths.snooze, legacy_snooze_json()).unwrap();
        let mut on_disk = IgnoreState::new();
        on_disk.ignore("https://github.com/o/r/pull/3".to_string(), Utc::now());
        fs::write(&paths.ignore, serde_json::to_string(&on_disk).unwrap()).unwrap();

        let (_, ignore) = load_hide_state(&paths).unwrap();

        assert!(ignore.is_ignored(URL_A), "converted legacy entry");
        assert!(
            ignore.is_ignored("https://github.com/o/r/pull/3"),
            "ignore.json entry"
        );
        assert_eq!(ignore.ignored.len(), 2);
    }

    // LOCKED: regression for the one-list invariant (feat/ignore).
    // A URL can only be in one hide list. If a crash between the two writes
    // leaves it in both files, ignored (the stronger hide) wins and the
    // snooze entry is dropped.
    #[test]
    fn ignored_wins_when_url_in_both_files() {
        let paths = temp_paths("both");
        let mut snooze = SnoozeState::new();
        snooze.snooze(URL_A.to_string(), Utc::now() + Duration::days(1));
        fs::write(&paths.snooze, serde_json::to_string(&snooze).unwrap()).unwrap();
        let mut ignore = IgnoreState::new();
        ignore.ignore(URL_A.to_string(), Utc::now());
        fs::write(&paths.ignore, serde_json::to_string(&ignore).unwrap()).unwrap();

        let (snooze, ignore) = load_hide_state(&paths).unwrap();

        assert!(ignore.is_ignored(URL_A));
        assert!(!snooze.snoozed.contains_key(URL_A));
    }

    #[test]
    fn save_writes_both_files_and_roundtrips() {
        let paths = temp_paths("roundtrip");
        let mut snooze = SnoozeState::new();
        snooze.snooze(URL_A.to_string(), Utc::now() + Duration::days(1));
        let mut ignore = IgnoreState::new();
        ignore.ignore(URL_B.to_string(), Utc::now());

        save_hide_state(&paths, &snooze, &ignore).unwrap();
        let (loaded_snooze, loaded_ignore) = load_hide_state(&paths).unwrap();

        assert!(paths.snooze.exists() && paths.ignore.exists());
        assert!(loaded_snooze.is_snoozed(URL_A));
        assert!(loaded_ignore.is_ignored(URL_B));
    }

    // LOCKED: regression for forward compatibility (feat/ignore).
    // pr-pal 1.x and pr-bro read snooze.json; the saved file must keep
    // version 1 and never contain a null wake time.
    #[test]
    fn saved_snooze_file_stays_v1_compatible() {
        let paths = temp_paths("v1-compat");
        let mut snooze = SnoozeState::new();
        snooze.snooze(URL_A.to_string(), Utc::now() + Duration::days(1));

        save_hide_state(&paths, &snooze, &IgnoreState::new()).unwrap();
        let text = fs::read_to_string(&paths.snooze).unwrap();
        let json: serde_json::Value = serde_json::from_str(&text).unwrap();

        assert_eq!(json["version"], 1);
        assert!(!text.contains("null"), "no null wake times: {text}");
        assert!(json["snoozed"][URL_A]["snooze_until"].is_string());
    }

    #[test]
    fn unsupported_ignore_version_is_an_error() {
        let paths = temp_paths("bad-version");
        fs::write(&paths.ignore, r#"{"version": 99, "ignored": {}}"#).unwrap();
        assert!(load_hide_state(&paths).is_err());
    }

    // --- partition_hidden ---

    fn pr(number: u64) -> PullRequest {
        PullRequest {
            title: format!("PR {number}"),
            number,
            author: "a".to_string(),
            repo: "o/r".to_string(),
            url: format!("https://github.com/o/r/pull/{number}"),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            additions: 0,
            deletions: 0,
            approvals: 0,
            draft: false,
            labels: vec![],
            user_has_reviewed: false,
            filtered_size: None,
            signals: Default::default(),
        }
    }

    /// A PR the suppression policy would hide as awaiting-author.
    fn awaiting_author_pr(number: u64) -> PullRequest {
        let mut pr = pr(number);
        pr.user_has_reviewed = true;
        pr.signals.my_last_review_at = Some(Utc::now() - Duration::days(3));
        pr
    }

    fn policy() -> SuppressPolicy {
        SuppressPolicy {
            wake_on: vec![WakeEvent::Push],
            resurface_after: Some(Duration::days(21)),
        }
    }

    fn numbers(prs: &[PullRequest]) -> Vec<u64> {
        prs.iter().map(|p| p.number).collect()
    }

    #[test]
    fn partition_routes_each_pr_to_one_list() {
        let mut snooze = SnoozeState::new();
        snooze.snooze(pr(2).url, Utc::now() + Duration::days(1));
        let mut ignore = IgnoreState::new();
        ignore.ignore(pr(4).url, Utc::now());

        let split = partition_hidden(
            vec![pr(1), pr(2), awaiting_author_pr(3), pr(4)],
            &snooze,
            &ignore,
            Some(&policy()),
            Utc::now(),
        );

        assert_eq!(numbers(&split.active), vec![1]);
        assert_eq!(numbers(&split.snoozed), vec![2]);
        assert_eq!(numbers(&split.suppressed), vec![3]);
        assert_eq!(numbers(&split.ignored), vec![4]);
    }

    // LOCKED: regression for hide precedence (feat/ignore).
    // Ignore outranks a manual snooze: a URL in both states is ignored only.
    #[test]
    fn ignore_outranks_snooze() {
        let mut snooze = SnoozeState::new();
        snooze.snooze(pr(1).url, Utc::now() + Duration::days(1));
        let mut ignore = IgnoreState::new();
        ignore.ignore(pr(1).url, Utc::now());

        let split = partition_hidden(vec![pr(1)], &snooze, &ignore, None, Utc::now());

        assert_eq!(numbers(&split.ignored), vec![1]);
        assert!(split.snoozed.is_empty());
        assert!(split.active.is_empty());
    }

    // LOCKED: regression for hide precedence (feat/ignore).
    // Ignore outranks suppression: an ignored PR awaiting its author is
    // ignored, not suppressed, so it cannot resurface on a wake event.
    #[test]
    fn ignore_outranks_suppression() {
        let mut ignore = IgnoreState::new();
        ignore.ignore(pr(1).url, Utc::now());

        let split = partition_hidden(
            vec![awaiting_author_pr(1)],
            &SnoozeState::new(),
            &ignore,
            Some(&policy()),
            Utc::now(),
        );

        assert_eq!(numbers(&split.ignored), vec![1]);
        assert!(split.suppressed.is_empty());
        assert!(split.active.is_empty());
    }

    #[test]
    fn ignored_list_is_oldest_ignore_first() {
        let now = Utc::now();
        let mut ignore = IgnoreState::new();
        ignore.ignore(pr(1).url, now - Duration::days(1));
        ignore.ignore(pr(2).url, now - Duration::days(30));
        ignore.ignore(pr(3).url, now - Duration::days(7));

        let split = partition_hidden(
            vec![pr(1), pr(2), pr(3)],
            &SnoozeState::new(),
            &ignore,
            None,
            now,
        );

        assert_eq!(numbers(&split.ignored), vec![2, 3, 1]);
    }
}
