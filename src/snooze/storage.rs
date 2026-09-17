use super::types::{SnoozeEntry, SnoozeState};
use anyhow::{Context, Result};
use atomic_write_file::AtomicWriteFile;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use std::collections::HashMap;
use std::fs::File;
use std::path::Path;

/// On-disk v1 entry. pr-bro and pr-pal 1.x wrote `snooze_until: null` for
/// an indefinite snooze; pr-pal 2.x reads that as an ignore.
#[derive(Deserialize)]
struct RawSnoozeEntry {
    snoozed_at: DateTime<Utc>,
    snooze_until: Option<DateTime<Utc>>,
}

#[derive(Deserialize)]
struct RawSnoozeState {
    version: u32,
    #[serde(default)]
    snoozed: HashMap<String, RawSnoozeEntry>,
}

/// Result of reading a v1 `snooze.json`.
pub struct LoadedSnooze {
    /// Timed snoozes.
    pub state: SnoozeState,
    /// Legacy indefinite snoozes (`snooze_until: null`) as `(url, snoozed_at)`.
    /// Callers convert these to ignores.
    pub legacy_indefinite: Vec<(String, DateTime<Utc>)>,
}

/// Read a v1 snooze file, splitting out legacy indefinite entries.
///
/// A missing file is an empty state. An unsupported version is an error.
pub fn load_snooze_file(path: &Path) -> Result<LoadedSnooze> {
    let mut loaded = LoadedSnooze {
        state: SnoozeState::new(),
        legacy_indefinite: Vec::new(),
    };
    if !path.exists() {
        return Ok(loaded);
    }

    let file = File::open(path)
        .with_context(|| format!("Failed to open snooze state file at {}", path.display()))?;
    let raw: RawSnoozeState =
        serde_json::from_reader(file).context("Failed to load snooze state")?;
    if raw.version != 1 {
        anyhow::bail!("Unsupported snooze state version: {}", raw.version);
    }

    for (url, entry) in raw.snoozed {
        match entry.snooze_until {
            Some(until) => {
                loaded.state.snoozed.insert(
                    url,
                    SnoozeEntry {
                        snoozed_at: entry.snoozed_at,
                        snooze_until: until,
                    },
                );
            }
            None => loaded.legacy_indefinite.push((url, entry.snoozed_at)),
        }
    }
    Ok(loaded)
}

/// Save snooze state to a JSON file atomically, creating the parent
/// directory if needed. The file stays in the v1 schema so pr-bro and
/// pr-pal 1.x can still read it.
pub fn save_snooze_state(path: &Path, state: &SnoozeState) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create directory {}", parent.display()))?;
    }

    let mut file = AtomicWriteFile::open(path)
        .with_context(|| format!("Failed to open atomic write file at {}", path.display()))?;
    serde_json::to_writer_pretty(&mut file, state).context("Failed to serialize snooze state")?;
    file.commit().context("Failed to save snooze state")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;
    use std::env;

    #[test]
    fn test_load_missing_file_returns_empty() {
        let temp_path = env::temp_dir().join("pr_pal_test_missing.json");
        let _ = std::fs::remove_file(&temp_path);

        let loaded = load_snooze_file(&temp_path).unwrap();
        assert_eq!(loaded.state.version, 1);
        assert!(loaded.state.snoozed.is_empty());
        assert!(loaded.legacy_indefinite.is_empty());
    }

    #[test]
    fn test_save_and_load_roundtrip() {
        let temp_path = env::temp_dir().join("pr_pal_test_roundtrip.json");
        let _ = std::fs::remove_file(&temp_path);

        let mut state = SnoozeState::new();
        let future = Utc::now() + Duration::hours(2);
        state.snooze("https://github.com/owner/repo/pull/1".to_string(), future);
        state.snooze("https://github.com/owner/repo/pull/2".to_string(), future);

        save_snooze_state(&temp_path, &state).unwrap();
        let loaded = load_snooze_file(&temp_path).unwrap();

        assert_eq!(loaded.state.version, 1);
        assert_eq!(loaded.state.snoozed.len(), 2);
        assert!(loaded
            .state
            .is_snoozed("https://github.com/owner/repo/pull/1"));
        assert!(loaded
            .state
            .is_snoozed("https://github.com/owner/repo/pull/2"));
        assert!(loaded.legacy_indefinite.is_empty());

        let _ = std::fs::remove_file(&temp_path);
    }

    #[test]
    fn legacy_null_until_is_split_out() {
        let temp_path = env::temp_dir().join("pr_pal_test_legacy_null.json");
        std::fs::write(
            &temp_path,
            r#"{"version":1,"snoozed":{
                "https://github.com/o/r/pull/1":{"snoozed_at":"2026-03-01T10:00:00Z","snooze_until":null},
                "https://github.com/o/r/pull/2":{"snoozed_at":"2026-03-02T10:00:00Z","snooze_until":"2999-01-01T00:00:00Z"}
            }}"#,
        )
        .unwrap();

        let loaded = load_snooze_file(&temp_path).unwrap();

        assert_eq!(loaded.state.snoozed.len(), 1);
        assert!(loaded.state.is_snoozed("https://github.com/o/r/pull/2"));
        assert_eq!(
            loaded.legacy_indefinite,
            vec![(
                "https://github.com/o/r/pull/1".to_string(),
                "2026-03-01T10:00:00Z".parse::<DateTime<Utc>>().unwrap()
            )]
        );

        let _ = std::fs::remove_file(&temp_path);
    }

    #[test]
    fn unsupported_version_is_an_error() {
        let temp_path = env::temp_dir().join("pr_pal_test_bad_version.json");
        std::fs::write(&temp_path, r#"{"version":2,"snoozed":{}}"#).unwrap();
        assert!(load_snooze_file(&temp_path).is_err());
        let _ = std::fs::remove_file(&temp_path);
    }
}
