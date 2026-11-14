use super::*;

#[test]
fn validate_approvals_str_accepts_bare_count() {
    // A "per N" where N is just a number (no time unit) means "per N
    // approvals" and must be rewritten to "per 1sec" before parsing.
    assert!(validate_approvals_str("+10 per 1").is_ok());
    assert!(validate_approvals_str("x0.8 per 1").is_ok());
}

#[test]
fn validate_approvals_str_accepts_decimal_count() {
    // Decimal counts (e.g. "per 1.5") are numeric-only too and should be
    // converted the same way as whole numbers.
    assert!(validate_approvals_str("+5 per 1.5").is_ok());
}

#[test]
fn validate_approvals_str_leaves_time_durations_untouched() {
    // A "per" part that is a real time duration (not a bare count) must
    // be passed through as-is and still parse successfully.
    assert!(validate_approvals_str("+1 per 1h").is_ok());
}

#[test]
fn validate_approvals_str_accepts_flat_effects() {
    assert!(validate_approvals_str("+20").is_ok());
    assert!(validate_approvals_str("x2").is_ok());
}

#[test]
fn validate_approvals_str_rejects_invalid_input() {
    let result = validate_approvals_str("invalid");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("must start with"));
}
