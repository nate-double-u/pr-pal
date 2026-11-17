pub mod prompt;

/// Environment variable name for providing a GitHub token
pub const ENV_TOKEN_VAR: &str = "PR_PAL_GH_TOKEN";

// Re-export prompt functions for convenience
pub use prompt::{prompt_for_token, reprompt_for_token, setup_token_if_missing};

/// Check for a GitHub token in the PR_PAL_GH_TOKEN environment variable.
/// Returns Some(token) if the env var is set and non-empty, None otherwise.
pub fn get_token_from_env() -> Option<String> {
    match std::env::var(ENV_TOKEN_VAR) {
        Ok(val) => {
            let trimmed = val.trim().to_string();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            }
        }
        Err(_) => None,
    }
}

#[cfg(test)]
#[path = "mod_test.rs"]
mod tests;
