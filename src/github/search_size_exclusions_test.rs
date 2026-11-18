use super::*;

fn file(name: &str, additions: u64, deletions: u64) -> (String, u64, u64) {
    (name.to_string(), additions, deletions)
}

#[test]
fn size_exclusions_sums_all_files_when_no_patterns_given() {
    let files = vec![file("src/main.rs", 10, 2), file("Cargo.lock", 100, 50)];
    let total = apply_size_exclusions(&files, &[]).expect("no patterns should not error");
    assert_eq!(total, 162);
}

#[test]
fn size_exclusions_excludes_files_matching_a_pattern() {
    let files = vec![file("src/main.rs", 10, 2), file("Cargo.lock", 100, 50)];
    let total = apply_size_exclusions(&files, &["*.lock".to_string()])
        .expect("valid pattern should not error");
    assert_eq!(total, 12);
}

#[test]
fn size_exclusions_match_on_basename_not_full_path() {
    // The pattern has no path separators, so it must match against the
    // file's basename even when the file lives in a subdirectory.
    let files = vec![
        file("vendor/pkg/Cargo.lock", 100, 50),
        file("src/lib.rs", 3, 1),
    ];
    let total = apply_size_exclusions(&files, &["*.lock".to_string()])
        .expect("valid pattern should not error");
    assert_eq!(total, 4);
}

#[test]
fn size_exclusions_empty_file_list_is_zero() {
    let total = apply_size_exclusions(&[], &["*.lock".to_string()])
        .expect("empty file list should not error");
    assert_eq!(total, 0);
}

#[test]
fn size_exclusions_invalid_glob_pattern_errors() {
    let files = vec![file("src/main.rs", 10, 2)];
    let result = apply_size_exclusions(&files, &["[".to_string()]);
    assert!(result.is_err(), "unterminated glob class should error");
}
