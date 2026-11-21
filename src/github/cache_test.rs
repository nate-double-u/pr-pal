use super::*;
use http::{HeaderMap, Uri};
use octocrab::service::middleware::cache::{CacheKey, CacheStorage};

fn unique_cache_path(test_name: &str) -> PathBuf {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("pr-pal-test-cache-{}-{}", test_name, timestamp))
}

// LOCKED: merge guard for the pr-pal cache namespace (PR #6). The fork
// caches under pr-pal/http-cache, not upstream's pr-bro; an upstream
// merge must not silently restore the old namespace.
#[test]
fn cache_path_is_pr_pal() {
    assert!(get_cache_path().ends_with("pr-pal/http-cache"));
}

// Shared guard for tests below that need get_cache_path() to resolve
// under a throwaway directory. get_cache_path()/clear_cache()/
// evict_stale_entries() are not parameterized, so the only way to
// exercise them without touching the real user cache dir is to point
// XDG_CACHE_HOME (which `dirs::cache_dir()` honors on Linux) at a temp
// dir for the duration of the test. Mirrors the ENV_LOCK pattern in
// src/credentials/mod.rs for the same reason: serialize access to a
// process-wide env var across tests in this binary.
static XDG_CACHE_LOCK: Mutex<()> = Mutex::new(());

fn with_xdg_cache_home<T>(dir: &std::path::Path, body: impl FnOnce() -> T) -> T {
    let _guard = XDG_CACHE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let previous = std::env::var("XDG_CACHE_HOME").ok();
    std::env::set_var("XDG_CACHE_HOME", dir);

    let result = body();

    match previous {
        Some(v) => std::env::set_var("XDG_CACHE_HOME", v),
        None => std::env::remove_var("XDG_CACHE_HOME"),
    }

    result
}

#[test]
fn clear_cache_removes_an_existing_cache_directory() {
    let xdg_home = unique_cache_path("clear-existing");
    let cache_dir = xdg_home.join("pr-pal/http-cache");
    std::fs::create_dir_all(&cache_dir).unwrap();
    std::fs::write(cache_dir.join("some-entry"), b"data").unwrap();
    assert!(cache_dir.exists());

    with_xdg_cache_home(&xdg_home, || {
        assert!(clear_cache().is_ok());
    });

    assert!(!cache_dir.exists());
    let _ = std::fs::remove_dir_all(&xdg_home);
}

#[test]
fn clear_cache_is_ok_when_directory_does_not_exist() {
    let xdg_home = unique_cache_path("clear-missing");
    // Deliberately do not create xdg_home/pr-pal/http-cache.

    with_xdg_cache_home(&xdg_home, || {
        assert!(clear_cache().is_ok());
    });

    let _ = std::fs::remove_dir_all(&xdg_home);
}

#[test]
fn evict_stale_entries_returns_zero_for_a_missing_cache_directory() {
    let xdg_home = unique_cache_path("evict-missing");

    let removed = with_xdg_cache_home(&xdg_home, evict_stale_entries);

    assert_eq!(removed, 0);
    let _ = std::fs::remove_dir_all(&xdg_home);
}

#[test]
fn evict_stale_entries_keeps_freshly_written_entries() {
    let xdg_home = unique_cache_path("evict-fresh");
    let cache_dir = xdg_home.join("pr-pal/http-cache");
    std::fs::create_dir_all(&cache_dir).unwrap();
    cacache::write_sync(&cache_dir, "fresh-key", b"fresh-value").unwrap();

    let removed = with_xdg_cache_home(&xdg_home, evict_stale_entries);

    assert_eq!(removed, 0);
    assert!(cacache::read_sync(&cache_dir, "fresh-key").is_ok());
    let _ = std::fs::remove_dir_all(&xdg_home);
}

#[test]
fn test_valid_json_is_cached() {
    let cache_path = unique_cache_path("valid");
    let cache = DiskCache::new(cache_path.clone());

    let uri = Uri::from_static("https://api.github.com/repos/test/test/pulls/1");
    let key = CacheKey::ETag("test-etag".to_string());
    let headers = HeaderMap::new();

    // Write valid JSON body
    let mut writer = cache.writer(&uri, key, headers);
    writer.write_body(br#"{"login":"test","id":1}"#);
    drop(writer);

    // Verify cache hit
    assert!(cache.try_hit(&uri).is_some());
    assert!(cache.load(&uri).is_some());

    // Cleanup
    let _ = std::fs::remove_dir_all(&cache_path);
}

#[test]
fn test_truncated_json_is_not_cached() {
    let cache_path = unique_cache_path("truncated");
    let cache = DiskCache::new(cache_path.clone());

    let uri = Uri::from_static("https://api.github.com/repos/test/test/pulls/2");
    let key = CacheKey::ETag("test-etag-2".to_string());
    let headers = HeaderMap::new();

    // Write truncated JSON body (missing closing brace and value)
    let mut writer = cache.writer(&uri, key, headers);
    writer.write_body(br#"{"login":"test","id":"#);
    drop(writer);

    // Verify cache miss - truncated JSON should not be cached
    assert!(cache.try_hit(&uri).is_none());
    assert!(cache.load(&uri).is_none());

    // Cleanup
    let _ = std::fs::remove_dir_all(&cache_path);
}

#[test]
fn test_empty_body_is_not_cached() {
    let cache_path = unique_cache_path("empty");
    let cache = DiskCache::new(cache_path.clone());

    let uri = Uri::from_static("https://api.github.com/repos/test/test/pulls/3");
    let key = CacheKey::ETag("test-etag-3".to_string());
    let headers = HeaderMap::new();

    // Write empty body
    let mut writer = cache.writer(&uri, key, headers);
    writer.write_body(b"");
    drop(writer);

    // Verify cache miss - empty body should not be cached
    assert!(cache.try_hit(&uri).is_none());
    assert!(cache.load(&uri).is_none());

    // Cleanup
    let _ = std::fs::remove_dir_all(&cache_path);
}
