use super::types::IgnoreState;
use anyhow::{Context, Result};
use atomic_write_file::AtomicWriteFile;
use std::fs::File;
use std::path::Path;

/// Load ignore state. A missing file is an empty state; an unsupported
/// version is an error.
pub fn load_ignore_state(path: &Path) -> Result<IgnoreState> {
    if !path.exists() {
        return Ok(IgnoreState::new());
    }

    let file = File::open(path)
        .with_context(|| format!("Failed to open ignore state file at {}", path.display()))?;
    let state: IgnoreState =
        serde_json::from_reader(file).context("Failed to load ignore state")?;
    if state.version != 1 {
        anyhow::bail!("Unsupported ignore state version: {}", state.version);
    }
    Ok(state)
}

/// Save ignore state atomically, creating the parent directory if needed.
pub fn save_ignore_state(path: &Path, state: &IgnoreState) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create directory {}", parent.display()))?;
    }

    let mut file = AtomicWriteFile::open(path)
        .with_context(|| format!("Failed to open atomic write file at {}", path.display()))?;
    serde_json::to_writer_pretty(&mut file, state).context("Failed to serialize ignore state")?;
    file.commit().context("Failed to save ignore state")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use std::env;

    #[test]
    fn missing_file_is_empty() {
        let temp_path = env::temp_dir().join("pr_pal_test_ignore_missing.json");
        let _ = std::fs::remove_file(&temp_path);
        let state = load_ignore_state(&temp_path).unwrap();
        assert_eq!(state.version, 1);
        assert!(state.ignored.is_empty());
    }

    #[test]
    fn roundtrip() {
        let temp_path = env::temp_dir().join("pr_pal_test_ignore_roundtrip.json");
        let _ = std::fs::remove_file(&temp_path);

        let mut state = IgnoreState::new();
        state.ignore("https://github.com/o/r/pull/1".to_string(), Utc::now());
        save_ignore_state(&temp_path, &state).unwrap();

        let loaded = load_ignore_state(&temp_path).unwrap();
        assert!(loaded.is_ignored("https://github.com/o/r/pull/1"));
        assert_eq!(loaded.ignored.len(), 1);

        let _ = std::fs::remove_file(&temp_path);
    }

    #[test]
    fn unsupported_version_is_an_error() {
        let temp_path = env::temp_dir().join("pr_pal_test_ignore_bad_version.json");
        std::fs::write(&temp_path, r#"{"version":99,"ignored":{}}"#).unwrap();
        assert!(load_ignore_state(&temp_path).is_err());
        let _ = std::fs::remove_file(&temp_path);
    }
}
