use super::*;
use super::test_support::with_token_env;

// LOCKED: merge guard for the pr-pal token env var (PR #6). The fork
// reads PR_PAL_GH_TOKEN; an upstream merge must not revert it to
// upstream's PR_BRO_GH_TOKEN.
#[test]
fn token_env_var_is_pr_pal() {
    assert_eq!(super::ENV_TOKEN_VAR, "PR_PAL_GH_TOKEN");
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
