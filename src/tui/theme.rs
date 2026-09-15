//! Centralized theme module for TUI color constants and styles

use ratatui::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Theme {
    Light,
    Dark,
}

/// Complete color palette for the TUI
#[derive(Debug, Clone)]
pub struct ThemeColors {
    // Score-based colors (traffic light pattern)
    pub score_high: Color,
    pub score_mid: Color,
    pub score_low: Color,

    // Score bar colors
    pub bar_filled_high: Color,
    pub bar_filled_mid: Color,
    pub bar_filled_low: Color,
    pub bar_empty: Color,

    // Table colors
    pub row_alt_bg: Color,
    pub index_color: Color,

    // Styles
    pub title_style: Style,
    pub header_style: Style,
    pub tab_active: Style,
    pub row_selected: Style,

    // General colors
    pub muted: Color,
    pub title_color: Color,

    // Tab colors
    pub tab_active_style: Style,
    pub tab_inactive_style: Style,

    // Status bar colors
    pub status_bar_bg: Color,
    pub status_key_color: Color,
    pub flash_success: Color,
    pub flash_error: Color,

    // Divider and separator colors
    pub divider_color: Color,

    // Popup overlay colors
    pub popup_border: Color,
    pub popup_title: Style,
    pub popup_bg: Color,

    // Scrollbar colors
    pub scrollbar_thumb: Color,
    pub scrollbar_track: Color,

    // Update banner colors
    pub banner_bg: Color,
    pub banner_fg: Color,
    pub banner_key: Color,
}

impl ThemeColors {
    /// Create a ThemeColors palette for the given theme
    pub fn new(theme: Theme) -> Self {
        match theme {
            Theme::Dark => Self::dark(),
            Theme::Light => Self::light(),
        }
    }

    /// Dark theme palette (reproduces original constants exactly)
    pub fn dark() -> Self {
        Self {
            score_high: Color::Red,
            score_mid: Color::Yellow,
            score_low: Color::Green,
            bar_filled_high: Color::Red,
            bar_filled_mid: Color::Yellow,
            bar_filled_low: Color::Green,
            bar_empty: Color::DarkGray,
            row_alt_bg: Color::Indexed(235),
            index_color: Color::DarkGray,
            title_style: Style::new().bold(),
            header_style: Style::new().bold(),
            tab_active: Style::new().reversed(),
            row_selected: Style::new().reversed(),
            muted: Color::Gray,
            title_color: Color::Cyan,
            tab_active_style: Style::new().fg(Color::Cyan).bold(),
            tab_inactive_style: Style::new().fg(Color::DarkGray),
            status_bar_bg: Color::Indexed(236),
            status_key_color: Color::Cyan,
            flash_success: Color::Green,
            flash_error: Color::Red,
            divider_color: Color::Indexed(238),
            popup_border: Color::Cyan,
            popup_title: Style::new().fg(Color::Cyan).bold(),
            popup_bg: Color::Indexed(234),
            scrollbar_thumb: Color::Indexed(244),
            scrollbar_track: Color::Indexed(236),
            banner_bg: Color::Rgb(50, 50, 120),
            banner_fg: Color::White,
            banner_key: Color::Yellow,
        }
    }

    /// Light theme palette (optimized for light terminal backgrounds)
    fn light() -> Self {
        Self {
            // Score colors stay the same (traffic light colors work on both)
            score_high: Color::Red,
            score_mid: Color::Yellow,
            score_low: Color::Green,
            bar_filled_high: Color::Red,
            bar_filled_mid: Color::Yellow,
            bar_filled_low: Color::Green,
            bar_empty: Color::Indexed(250), // Lighter empty bar
            // Table colors adjusted for light background
            row_alt_bg: Color::Indexed(254),  // Light gray
            index_color: Color::Indexed(240), // Medium gray for contrast
            // Styles stay the same (bold/reversed work universally)
            title_style: Style::new().bold(),
            header_style: Style::new().bold(),
            tab_active: Style::new().reversed(),
            row_selected: Style::new().reversed(),
            // General colors adjusted
            muted: Color::Indexed(244), // Darker muted for readability
            title_color: Color::Blue,   // Blue instead of cyan
            // Tab colors adjusted
            tab_active_style: Style::new().fg(Color::Blue).bold(),
            tab_inactive_style: Style::new().fg(Color::Indexed(240)),
            // Status bar adjusted
            status_bar_bg: Color::Indexed(253), // Light background
            status_key_color: Color::Blue,
            // Flash colors stay the same
            flash_success: Color::Green,
            flash_error: Color::Red,
            // Divider adjusted
            divider_color: Color::Indexed(250), // Lighter divider
            // Popup adjusted
            popup_border: Color::Blue,
            popup_title: Style::new().fg(Color::Blue).bold(),
            popup_bg: Color::Indexed(255), // Near-white background
            // Scrollbar adjusted
            scrollbar_thumb: Color::Indexed(240),
            scrollbar_track: Color::Indexed(253),
            // Banner adjusted
            banner_bg: Color::Rgb(180, 180, 230), // Lighter blue-purple
            banner_fg: Color::Black,              // Dark text on light banner
            banner_key: Color::Indexed(88),       // Dark red for highlight
        }
    }

    /// Returns the color for a score's tier within the given distribution
    pub fn tier_color(&self, score: f64, tiers: &ScoreTiers) -> Color {
        if score <= 0.0 {
            // Floored scores are the lowest priority; their bars are empty.
            Color::Reset
        } else if score >= tiers.hot {
            self.score_high
        } else if score > tiers.warm {
            self.score_mid
        } else if score >= tiers.cold {
            self.score_low
        } else {
            Color::Reset
        }
    }
}

/// Distribution-based score tiers computed with head/tail breaks
/// (Jiang 2013), a classification scheme for heavy-tailed data.
///
/// Scores are products of multiplicative modifiers, so a queue is heavy
/// tailed: means split it far more honestly than fractions of the max.
/// `warm` is the mean of all scores; `hot` recursively takes the mean of the
/// above-mean head (up to three levels), converging on genuine outliers;
/// `cold` is the mean of the at-or-below-mean tail, the noise floor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScoreTiers {
    hot: f64,
    warm: f64,
    cold: f64,
}

impl ScoreTiers {
    pub fn from_scores(scores: &[f64]) -> Self {
        if scores.is_empty() {
            return Self {
                hot: f64::INFINITY,
                warm: f64::INFINITY,
                cold: f64::INFINITY,
            };
        }
        let mean = |xs: &[f64]| xs.iter().sum::<f64>() / xs.len() as f64;
        let warm = mean(scores);
        let mut hot = warm;
        let mut head: Vec<f64> = scores.iter().copied().filter(|s| *s > hot).collect();
        for _ in 0..2 {
            if head.is_empty() {
                break;
            }
            hot = mean(&head);
            head.retain(|s| *s > hot);
        }
        let tail: Vec<f64> = scores.iter().copied().filter(|s| *s <= warm).collect();
        let cold = if tail.is_empty() { warm } else { mean(&tail) };
        Self { hot, warm, cold }
    }
}

/// How many decades below the max score the display gradient spans.
const SCORE_WINDOW_DECADES: f64 = 3.0;

/// Relative intensity of a score in [0, 1], log-scaled.
///
/// Scores are products of multiplicative modifiers, so a list spreads across
/// orders of magnitude and linear score/max lets one outlier flatten every
/// other row. Intensity instead falls linearly with decades below the max:
/// 1.0 at the max, 0.0 at 1000x below or worse.
pub fn score_intensity(score: f64, max_score: f64) -> f64 {
    if max_score <= 0.0 || score <= 0.0 {
        return 0.0;
    }
    let decades_below = (max_score / score).log10();
    (1.0 - decades_below / SCORE_WINDOW_DECADES).clamp(0.0, 1.0)
}

/// Resolve theme from config string ("dark", "light", "auto")
pub fn resolve_theme(config_theme: &str) -> Theme {
    match config_theme {
        "light" => Theme::Light,
        "dark" => Theme::Dark,
        "auto" => detect_terminal_theme(),
        _ => Theme::Dark, // Unknown value defaults to dark
    }
}

/// Detect terminal theme from background luminance
fn detect_terminal_theme() -> Theme {
    match terminal_light::luma() {
        Ok(luma) if luma > 0.5 => Theme::Light,
        _ => Theme::Dark, // Detection failed or dark background -> default dark
    }
}

#[cfg(test)]
mod tests {
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
            191.8, 62.7, 53.0, 52.6, 31.5, 26.8, 20.8, 18.4, 18.2, 16.2, 15.8, 15.0, 15.0, 14.8,
            13.9, 13.4, 13.0, 12.9, 12.9, 11.4, 10.7, 10.1, 9.4, 9.2, 8.3, 7.6, 7.3, 5.7, 4.8, 4.6,
            3.6, 3.4, 2.9, 2.7, 2.6, 2.5, 2.1, 1.8, 1.7, 1.6, 1.5, 1.4, 1.1, 1.1, 0.9, 0.7,
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
            26.6, 20.8, 18.3, 15.0, 15.0, 14.8, 13.3, 12.9, 12.9, 10.6, 10.6, 10.0, 9.2, 7.9, 7.5,
            7.3, 7.0, 6.9, 6.3, 5.7, 5.7, 4.8, 4.6, 4.5, 4.1, 4.1, 3.7, 3.6, 3.4, 3.1, 2.9, 2.7,
            2.6, 2.6, 2.5, 2.1, 1.8, 1.7, 1.6, 1.5, 1.4, 1.3, 1.1, 1.1, 1.1, 0.92, 0.684,
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
}
