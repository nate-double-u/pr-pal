use super::*;

#[test]
fn intensity_is_log_scaled_over_three_decades() {
    let max = 191_200.0;
    // At the max: full intensity.
    assert!((score_intensity(max, max) - 1.0).abs() < 1e-9);
    // ~3x below max is still near-tied in multiplicative terms.
    assert!(score_intensity(62_600.0, max) > 0.8);
    // One decade below: two thirds.
    assert!((score_intensity(19_120.0, max) - 2.0 / 3.0).abs() < 1e-9);
    // Three decades below: floor.
    assert!(score_intensity(191.2, max).abs() < 1e-9);
    // Beyond the window clamps to zero, never negative.
    assert_eq!(score_intensity(1.0, max), 0.0);
}

#[test]
fn intensity_handles_degenerate_inputs() {
    assert_eq!(score_intensity(0.0, 100.0), 0.0);
    assert_eq!(score_intensity(-5.0, 100.0), 0.0);
    assert_eq!(score_intensity(50.0, 0.0), 0.0);
    assert_eq!(score_intensity(50.0, -1.0), 0.0);
    // A score above max clamps to full rather than overflowing.
    assert_eq!(score_intensity(200.0, 100.0), 1.0);
}

// LOCKED: regression for score gradient collapse under outliers (pr-pal feedback).
// Head/tail tiers: only genuine outliers go red. With a 191.8k outlier,
// 62.7k is yellow (it was red under both linear percent-of-max and a fixed
// log window), and the below-tail-mean noise floor is uncolored.
#[test]
fn tier_colors_isolate_outliers_and_fade_the_tail() {
    let colors = ThemeColors::dark();
    // Real outlier-day distribution (thousands omitted).
    let pool = [
        191.8, 62.7, 53.0, 52.6, 31.5, 26.8, 20.8, 18.4, 18.2, 16.2, 15.8, 15.0, 15.0, 14.8, 13.9,
        13.4, 13.0, 12.9, 12.9, 11.4, 10.7, 10.1, 9.4, 9.2, 8.3, 7.6, 7.3, 5.7, 4.8, 4.6, 3.6, 3.4,
        2.9, 2.7, 2.6, 2.5, 2.1, 1.8, 1.7, 1.6, 1.5, 1.4, 1.1, 1.1, 0.9, 0.7,
    ];
    let tiers = ScoreTiers::from_scores(&pool);
    // hot = 90.025: the outlier alone is red.
    assert_eq!(colors.tier_color(191.8, &tiers), colors.score_high);
    assert_eq!(colors.tier_color(62.7, &tiers), colors.score_mid);
    // warm = 16.074: above the mean is yellow, at or below is not.
    assert_eq!(colors.tier_color(16.2, &tiers), colors.score_mid);
    assert_eq!(colors.tier_color(15.8, &tiers), colors.score_low);
    // cold = 6.872: at or above the tail mean is green, below is uncolored.
    assert_eq!(colors.tier_color(7.3, &tiers), colors.score_low);
    assert_eq!(colors.tier_color(5.7, &tiers), Color::Reset);
    assert_eq!(colors.tier_color(0.7, &tiers), Color::Reset);
}

#[test]
fn tiers_split_smooth_lists_into_small_head() {
    let colors = ThemeColors::dark();
    // A no-outlier day: hot = 16.622, warm = 6.592, cold = 2.869.
    let pool = [
        26.6, 20.8, 18.3, 15.0, 15.0, 14.8, 13.3, 12.9, 12.9, 10.6, 10.6, 10.0, 9.2, 7.9, 7.5, 7.3,
        7.0, 6.9, 6.3, 5.7, 5.7, 4.8, 4.6, 4.5, 4.1, 4.1, 3.7, 3.6, 3.4, 3.1, 2.9, 2.7, 2.6, 2.6,
        2.5, 2.1, 1.8, 1.7, 1.6, 1.5, 1.4, 1.3, 1.1, 1.1, 1.1, 0.92, 0.684,
    ];
    let tiers = ScoreTiers::from_scores(&pool);
    assert_eq!(colors.tier_color(26.6, &tiers), colors.score_high);
    assert_eq!(colors.tier_color(18.3, &tiers), colors.score_high);
    assert_eq!(colors.tier_color(15.0, &tiers), colors.score_mid);
    assert_eq!(colors.tier_color(6.9, &tiers), colors.score_mid);
    assert_eq!(colors.tier_color(6.3, &tiers), colors.score_low);
    assert_eq!(colors.tier_color(2.9, &tiers), colors.score_low);
    assert_eq!(colors.tier_color(2.7, &tiers), Color::Reset);
}

#[test]
fn tiers_handle_tiny_and_empty_pools() {
    let colors = ThemeColors::dark();
    // Two rows: the leader is red, the other stays green (not uncolored).
    let tiers = ScoreTiers::from_scores(&[100.0, 10.0]);
    assert_eq!(colors.tier_color(100.0, &tiers), colors.score_high);
    assert_eq!(colors.tier_color(10.0, &tiers), colors.score_low);
    // A single row is the top of its own distribution.
    let tiers = ScoreTiers::from_scores(&[42.0]);
    assert_eq!(colors.tier_color(42.0, &tiers), colors.score_high);
    // An empty pool colors nothing.
    let tiers = ScoreTiers::from_scores(&[]);
    assert_eq!(colors.tier_color(5.0, &tiers), Color::Reset);
}

// LOCKED: regression for all-red equal-score pools (pr-pal#3 Copilot review).
// When every score in a multi-row pool ties, the head/tail head is empty
// and hot collapses onto warm; nothing stands out, so no row may be red.
// Tied rows are the baseline: they land green.
#[test]
fn equal_score_pools_have_no_outliers() {
    let colors = ThemeColors::dark();
    let tiers = ScoreTiers::from_scores(&[50.0, 50.0, 50.0]);
    assert_eq!(colors.tier_color(50.0, &tiers), colors.score_low);
    let tiers = ScoreTiers::from_scores(&[7.5, 7.5]);
    assert_eq!(colors.tier_color(7.5, &tiers), colors.score_low);
}

// LOCKED: regression for non-finite score pools (pr-pal#3 Copilot review).
// MultiplyPerUnit scoring is unbounded powf: with PRs as old as GitHub
// (~18 years, ~158k hours) even multiply 1.005/hour overflows f64 to
// +inf, and huge finite scores can overflow a naive pool sum. Neither
// may poison the thresholds or bars of the finite rows.
#[test]
fn non_finite_scores_do_not_poison_the_pool() {
    let colors = ThemeColors::dark();
    // An overflowed row tops the queue; finite rows still tier normally.
    let tiers = ScoreTiers::from_scores(&[f64::INFINITY, f64::NAN, 100.0, 10.0]);
    assert_eq!(colors.tier_color(f64::INFINITY, &tiers), colors.score_high);
    assert_eq!(colors.tier_color(100.0, &tiers), colors.score_high);
    assert_eq!(colors.tier_color(10.0, &tiers), colors.score_low);
    assert_eq!(colors.tier_color(f64::NAN, &tiers), Color::Reset);
    // Huge finite scores must not overflow the mean into infinity.
    let tiers = ScoreTiers::from_scores(&[1e308, 1e308, 1.0]);
    assert_eq!(colors.tier_color(1e308, &tiers), colors.score_high);
    assert_eq!(colors.tier_color(1.0, &tiers), colors.score_low);
    // Bars: an overflowed score fills fully; an infinite max (avoided by
    // pooling only finite maxes in the ui) blanks finite rows.
    assert_eq!(score_intensity(f64::INFINITY, 100.0), 1.0);
    assert_eq!(score_intensity(100.0, f64::INFINITY), 0.0);
    assert_eq!(score_intensity(f64::NAN, 100.0), 0.0);
}

// LOCKED: regression for zero-score tier coloring (pr-pal#3 Copilot review).
// The engine floors scores at 0.0; a zero score is the lowest possible
// priority and its bar is empty, so it must stay uncolored even when
// tier thresholds collapse to zero.
#[test]
fn zero_scores_stay_uncolored() {
    let colors = ThemeColors::dark();
    // Tail mean of zero: a zero row must not turn green.
    let tiers = ScoreTiers::from_scores(&[100.0, 0.0, 0.0]);
    assert_eq!(colors.tier_color(0.0, &tiers), Color::Reset);
    // All-zero pool: thresholds collapse to zero; rows must not go red.
    let tiers = ScoreTiers::from_scores(&[0.0, 0.0]);
    assert_eq!(colors.tier_color(0.0, &tiers), Color::Reset);
}

// resolve_theme() had zero test coverage before this change (grep for
// `resolve_theme` showed only its definition and call sites in
// tui/mod.rs and main.rs). The "auto" branch depends on
// `detect_terminal_theme()`'s terminal-background detection, which is
// environment-dependent (no real terminal in CI) and not covered here
// for the same reason the TUI's own timing-based tests avoid asserting
// on environment-sensitive behavior; the three deterministic branches
// below are covered directly.
#[test]
fn resolve_theme_light_is_explicit() {
    assert_eq!(resolve_theme("light"), Theme::Light);
}

#[test]
fn resolve_theme_dark_is_explicit() {
    assert_eq!(resolve_theme("dark"), Theme::Dark);
}

#[test]
fn resolve_theme_unknown_value_defaults_to_dark() {
    assert_eq!(resolve_theme("not-a-real-theme"), Theme::Dark);
    assert_eq!(resolve_theme(""), Theme::Dark);
}
