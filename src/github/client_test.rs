use super::*;

// `Octocrab::builder().build()` spawns a tower buffer worker that requires an
// active Tokio runtime, so these tests must run inside one.
#[tokio::test]
async fn create_client_with_cache_disabled_returns_no_cache_handle() {
    let cache_config = CacheConfig { enabled: false };
    let (_, cache_handle) = create_client("fake-token", &cache_config)
        .expect("client creation should not fail without network access");
    assert!(
        cache_handle.is_none(),
        "cache handle must be None when caching is disabled"
    );
}

#[tokio::test]
async fn create_client_with_cache_enabled_returns_cache_handle() {
    let cache_config = CacheConfig { enabled: true };
    let (_, cache_handle) = create_client("fake-token", &cache_config)
        .expect("client creation should not fail without network access");
    assert!(
        cache_handle.is_some(),
        "cache handle must be Some when caching is enabled"
    );
}
