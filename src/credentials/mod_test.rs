use super::*;
use std::sync::Mutex;

// LOCKED: merge guard for the pr-pal token env var (PR #6). The fork
// reads PR_PAL_GH_TOKEN; an upstream merge must not revert it to
// upstream's PR_BRO_GH_TOKEN.
#[test]
fn token_env_var_is_pr_pal() {
    assert_eq!(super::ENV_TOKEN_VAR, "PR_PAL_GH_TOKEN");
}

// Serializes access to the process-wide PR_PAL_GH_TOKEN env var so these
// tests don't race with each other when run in parallel threads.
static ENV_LOCK: Mutex<()> = Mutex::new(());

/// Runs `body` with PR_PAL_GH_TOKEN set to `value` (or removed if `None`),
/// restoring the prior value afterward regardless of panics.
fn with_token_env<T>(value: Option<&str>, body: impl FnOnce() -> T) -> T {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let previous = std::env::var(ENV_TOKEN_VAR).ok();

    match value {
        Some(v) => std::env::set_var(ENV_TOKEN_VAR, v),
        None => std::env::remove_var(ENV_TOKEN_VAR),
    }

    let result = body();

    match previous {
        Some(v) => std::env::set_var(ENV_TOKEN_VAR, v),
        None => std::env::remove_var(ENV_TOKEN_VAR),
    }

    result
}

#[test]
fn get_token_from_env_returns_none_when_unset() {
    with_token_env(None, || {
        assert_eq!(get_token_from_env(), None);
    });
}

#[test]
fn get_token_from_env_returns_some_trimmed_value_when_set() {
    with_token_env(Some("  my-token  "), || {
        assert_eq!(get_token_from_env(), Some("my-token".to_string()));
    });
}

#[test]
fn get_token_from_env_returns_none_when_set_to_empty_or_whitespace() {
    with_token_env(Some("   "), || {
        assert_eq!(get_token_from_env(), None);
    });
}
