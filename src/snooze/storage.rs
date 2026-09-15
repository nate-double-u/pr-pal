use super::types::SnoozeState;
use anyhow::{Context, Result};
use atomic_write_file::AtomicWriteFile;
use std::fs::File;
use std::path::{Path, PathBuf};

/// Get the default snooze state file path (~/.config/pr-pal/snooze.json)
pub fn get_snooze_path() -> PathBuf {
    crate::config::get_config_dir().join("snooze.json")
}

/// Load snooze state from a JSON file
///
/// If the file doesn't exist, returns a new empty state.
/// If the file exists but has an unsupported version, returns an error.
pub fn load_snooze_state(path: &Path) -> Result<SnoozeState> {
    if !path.exists() {
        return Ok(SnoozeState::new());
    }

    let file = File::open(path)
        .with_context(|| format!("Failed to open snooze state file at {}", path.display()))?;

    let state: SnoozeState =
        serde_json::from_reader(file).context("Failed to load snooze state")?;

    // Version check
    if state.version != 1 {
        anyhow::bail!("Unsupported snooze state version: {}", state.version);
    }

    Ok(state)
}

/// Save snooze state to a JSON file atomically
///
/// Uses atomic-write-file to ensure the file is never left in a corrupted state.
/// Creates the config directory if it doesn't exist.
pub fn save_snooze_state(path: &Path, state: &SnoozeState) -> Result<()> {
    // Ensure config directory exists
    crate::config::ensure_config_dir()?;

    // Open atomic write file
    let mut file = AtomicWriteFile::open(path)
        .with_context(|| format!("Failed to open atomic write file at {}", path.display()))?;

    // Write JSON with pretty formatting
    serde_json::to_writer_pretty(&mut file, state).context("Failed to serialize snooze state")?;

    // Commit the write atomically
    file.commit().context("Failed to save snooze state")?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, Utc};
    use std::env;

    #[test]
    fn test_load_missing_file_returns_empty() {
        let temp_path = env::temp_dir().join("pr_bro_test_missing.json");
        // Ensure it doesn't exist
        let _ = std::fs::remove_file(&temp_path);

        let state = load_snooze_state(&temp_path).unwrap();
        assert_eq!(state.version, 1);
        assert!(state.snoozed.is_empty());
    }

    #[test]
    fn test_save_and_load_roundtrip() {
        let temp_path = env::temp_dir().join("pr_bro_test_roundtrip.json");
        // Ensure clean state
        let _ = std::fs::remove_file(&temp_path);

        // Create state with some data
        let mut state = SnoozeState::new();
        let future = Utc::now() + Duration::hours(2);
        state.snooze("https://github.com/owner/repo/pull/1".to_string(), None);
        state.snooze(
            "https://github.com/owner/repo/pull/2".to_string(),
            Some(future),
        );

        // Save
        save_snooze_state(&temp_path, &state).unwrap();

        // Load
        let loaded = load_snooze_state(&temp_path).unwrap();

        // Verify
        assert_eq!(loaded.version, 1);
        assert_eq!(loaded.snoozed.len(), 2);
        assert!(loaded.is_snoozed("https://github.com/owner/repo/pull/1"));
        assert!(loaded.is_snoozed("https://github.com/owner/repo/pull/2"));

        // Cleanup
        let _ = std::fs::remove_file(&temp_path);
    }
}
