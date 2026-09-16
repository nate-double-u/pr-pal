//! Persistence for the two hide files in the config dir: `snooze.json`
//! (timed snoozes) and `ignore.json` (permanent ignores).
//!
//! `snooze.json` keeps its pr-bro / pr-pal 1.x schema so older readers still
//! load it; the one legacy shape, `snooze_until: null` (an indefinite
//! snooze), is read as an ignore because that was always its intent. Saving
//! writes both files together so they cannot drift apart.

use crate::ignore::{IgnoreEntry, IgnoreState};
use crate::snooze::SnoozeState;
use anyhow::Result;
use std::path::{Path, PathBuf};

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
    use chrono::{DateTime, Duration, Utc};
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
}
