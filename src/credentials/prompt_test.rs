use super::*;
use crate::credentials::test_support::with_token_env;

// setup_token_if_missing() has two branches: env-var-set (deterministic,
// no I/O) and env-var-unset (falls through to an interactive stdin prompt
// via prompt_for_token/rpassword, which isn't testable without a stdin
// double). Only the env-var-set branch is covered here.
#[test]
fn setup_token_if_missing_returns_env_token_without_prompting() {
    with_token_env(Some("  env-token  "), || {
        let result = setup_token_if_missing();
        assert_eq!(result.unwrap(), "env-token");
    });
}
