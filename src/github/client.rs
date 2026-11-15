use super::cache::{get_cache_path, CacheConfig, DiskCache};
use anyhow::{Context, Result};
use octocrab::Octocrab;
use std::sync::Arc;

/// Create an authenticated GitHub client using a personal access token.
/// Returns both the Octocrab client and an optional DiskCache handle for manual cache control.
pub fn create_client(
    token: &str,
    cache_config: &CacheConfig,
) -> Result<(Octocrab, Option<Arc<DiskCache>>)> {
    let mut builder = Octocrab::builder().personal_token(token.to_string());

    let cache_handle = if cache_config.enabled {
        let cache_path = get_cache_path();
        let disk_cache = DiskCache::new(cache_path);
        let cache_handle = Arc::new(disk_cache.clone());
        builder = builder.cache(disk_cache);
        Some(cache_handle)
    } else {
        None
    };

    let client = builder.build().context("Failed to create GitHub client")?;
    Ok((client, cache_handle))
}

#[cfg(test)]
#[path = "client_test.rs"]
mod tests;
