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
        // Scoring multipliers are unbounded, so an overflowed +inf (or NaN)
        // score must not poison the thresholds for the finite rows.
        let finite: Vec<f64> = scores.iter().copied().filter(|s| s.is_finite()).collect();
        if finite.is_empty() {
            return Self {
                hot: f64::INFINITY,
                warm: f64::INFINITY,
                cold: f64::INFINITY,
            };
        }
        // Running mean: immune to sum overflow from huge finite scores.
        let mean = |xs: &[f64]| {
            xs.iter()
                .enumerate()
                .fold(0.0, |m, (i, x)| m + (x - m) / (i as f64 + 1.0))
        };
        let warm = mean(&finite);
        let mut hot = warm;
        let mut head: Vec<f64> = finite.iter().copied().filter(|s| *s > hot).collect();
        // An empty head with several rows means every score ties the mean:
        // there is no outlier to isolate. A single row is still the top of
        // its own distribution and stays hot.
        if head.is_empty() && finite.len() > 1 {
            hot = f64::INFINITY;
        }
        for _ in 0..2 {
            if head.is_empty() {
                break;
            }
            hot = mean(&head);
            head.retain(|s| *s > hot);
        }
        let tail: Vec<f64> = finite.iter().copied().filter(|s| *s <= warm).collect();
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
    // An overflowed score pins to full intensity; any other non-finite
    // input renders an empty bar.
    if score == f64::INFINITY {
        return 1.0;
    }
    if !score.is_finite() || !max_score.is_finite() || max_score <= 0.0 || score <= 0.0 {
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
#[path = "theme_test.rs"]
mod tests;
