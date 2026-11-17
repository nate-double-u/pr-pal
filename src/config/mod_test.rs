use super::*;
use std::env;

// LOCKED: merge guard for the pr-pal config path (PR #6). The fork's
// config lives under pr-pal, not upstream's pr-bro. An upstream merge
// must not silently move it back.
#[test]
fn config_dir_is_pr_pal() {
    assert!(super::get_config_dir().ends_with(".config/pr-pal"));
}

fn temp_config_path(name: &str) -> PathBuf {
    env::temp_dir().join(format!(
        "pr_pal_test_load_config_{}_{}.yaml",
        std::process::id(),
        name
    ))
}

#[test]
fn missing_file_is_an_error() {
    let path = temp_config_path("missing");
    let _ = fs::remove_file(&path);

    let err = load_config(Some(path.clone())).unwrap_err();
    assert!(err.to_string().contains("Config file not found"));
}

#[test]
fn unparseable_yaml_is_an_error() {
    let path = temp_config_path("bad_yaml");
    fs::write(&path, "queries: [not, a, mapping").unwrap();

    let err = load_config(Some(path.clone())).unwrap_err();
    assert!(err.to_string().contains("Failed to parse config"));

    let _ = fs::remove_file(&path);
}

#[test]
fn valid_file_loads_successfully() {
    let path = temp_config_path("valid");
    fs::write(
        &path,
        "queries:\n  - query: \"review-requested:@me is:open\"\n",
    )
    .unwrap();

    let config = load_config(Some(path.clone())).unwrap();
    assert_eq!(config.queries.len(), 1);
    assert_eq!(config.queries[0].query, "review-requested:@me is:open");
    assert_eq!(config.auto_refresh_interval, 300);
    assert_eq!(config.theme, "auto");

    let _ = fs::remove_file(&path);
}
