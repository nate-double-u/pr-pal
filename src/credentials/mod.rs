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

// Shared test helper for serializing access to the process-wide
// PR_PAL_GH_TOKEN env var across this module's test files (mod_test.rs,
// prompt_test.rs), which run in the same test binary and could otherwise
// race on the same env var.
#[cfg(test)]
pub(crate) mod test_support {
    use super::ENV_TOKEN_VAR;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    /// Runs `body` with PR_PAL_GH_TOKEN set to `value` (or removed if `None`),
    /// restoring the prior value afterward regardless of panics.
    pub(crate) fn with_token_env<T>(value: Option<&str>, body: impl FnOnce() -> T) -> T {
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
}

#[cfg(test)]
#[path = "mod_test.rs"]
mod tests;
