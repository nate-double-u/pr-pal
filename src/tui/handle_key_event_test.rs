use super::*;
use crate::config::Config;
use crate::github::cache::CacheConfig;
use crate::hide::HidePaths;
use crate::ignore::IgnoreState;
use crate::scoring::ScoreResult;
use crate::snooze::SnoozeState;
use app::{InputMode, View};

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn key_ctrl(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::CONTROL)
}

fn test_app(name: &str) -> App {
    App::new(
        Vec::new(),
        Vec::new(),
        SnoozeState::new(),
        IgnoreState::new(),
        HidePaths::in_dir(&std::env::temp_dir().join(format!(
            "pr-pal-tui-mod-test-{}-{}",
            std::process::id(),
            name
        ))),
        Config {
            scoring: None,
            queries: vec![],
            auto_refresh_interval: 300,
            theme: "dark".to_string(),
            suppress: None,
        },
        CacheConfig { enabled: false },
        None,
        false,
        Some("me".to_string()),
        Theme::Dark,
    )
}

fn app_with_rows(name: &str, n: usize) -> App {
    let mut app = test_app(name);
    app.active_prs = (0..n)
        .map(|i| {
            let mut pr = crate::github::types::PullRequest {
                title: format!("PR {i}"),
                number: i as u64,
                author: "author".to_string(),
                repo: "o/r".to_string(),
                url: format!("https://x/{i}"),
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
                additions: 1,
                deletions: 1,
                approvals: 0,
                draft: false,
                labels: vec![],
                user_has_reviewed: false,
                filtered_size: None,
                signals: Default::default(),
            };
            pr.number = i as u64 + 1;
            (
                pr,
                ScoreResult {
                    score: 100.0 - i as f64,
                    ..Default::default()
                },
            )
        })
        .collect();
    app.table_state.select(Some(0));
    app
}

#[test]
fn normal_mode_q_quits() {
    let mut app = test_app("q-quits");
    handle_key_event(&mut app, key(KeyCode::Char('q')));
    assert!(app.should_quit);
}

#[test]
fn normal_mode_ctrl_c_quits() {
    let mut app = test_app("ctrl-c-quits");
    handle_key_event(&mut app, key_ctrl(KeyCode::Char('c')));
    assert!(app.should_quit);
}

#[test]
fn normal_mode_navigation_moves_selection() {
    let mut app = app_with_rows("nav", 3);
    handle_key_event(&mut app, key(KeyCode::Char('j')));
    assert_eq!(app.table_state.selected(), Some(1));
    handle_key_event(&mut app, key(KeyCode::Down));
    assert_eq!(app.table_state.selected(), Some(2));
    handle_key_event(&mut app, key(KeyCode::Char('k')));
    assert_eq!(app.table_state.selected(), Some(1));
    handle_key_event(&mut app, key(KeyCode::Up));
    assert_eq!(app.table_state.selected(), Some(0));
}

#[test]
fn normal_mode_tab_switches_view() {
    let mut app = test_app("tab-switch");
    assert_eq!(app.current_view, View::Active);
    handle_key_event(&mut app, key(KeyCode::Tab));
    assert_eq!(app.current_view, View::Snoozed);
    handle_key_event(&mut app, key(KeyCode::BackTab));
    assert_eq!(app.current_view, View::Active);
}

#[test]
fn normal_mode_r_forces_refresh_with_flash() {
    let mut app = test_app("force-refresh");
    handle_key_event(&mut app, key(KeyCode::Char('r')));
    assert!(app.needs_refresh);
    assert!(app.force_refresh);
    assert!(app.flash_message.is_some());
}

#[test]
fn normal_mode_question_mark_shows_help() {
    let mut app = test_app("show-help");
    handle_key_event(&mut app, key(KeyCode::Char('?')));
    assert_eq!(app.input_mode, InputMode::Help);
}

#[test]
fn normal_mode_b_shows_score_breakdown_only_with_selection() {
    let mut app = test_app("no-selection-breakdown");
    handle_key_event(&mut app, key(KeyCode::Char('b')));
    assert_eq!(app.input_mode, InputMode::Normal);

    let mut app = app_with_rows("with-selection-breakdown", 1);
    handle_key_event(&mut app, key(KeyCode::Char('b')));
    assert_eq!(app.input_mode, InputMode::ScoreBreakdown);
}

#[test]
fn normal_mode_s_starts_snooze_input() {
    let mut app = app_with_rows("start-snooze", 1);
    handle_key_event(&mut app, key(KeyCode::Char('s')));
    assert_eq!(app.input_mode, InputMode::SnoozeInput);
}

#[test]
fn snooze_input_mode_chars_backspace_and_cancel() {
    let mut app = app_with_rows("snooze-typing", 1);
    app.input_mode = InputMode::SnoozeInput;

    handle_key_event(&mut app, key(KeyCode::Char('1')));
    handle_key_event(&mut app, key(KeyCode::Char('d')));
    assert_eq!(app.snooze_input, "1d");

    handle_key_event(&mut app, key(KeyCode::Backspace));
    assert_eq!(app.snooze_input, "1");

    handle_key_event(&mut app, key(KeyCode::Esc));
    assert_eq!(app.input_mode, InputMode::Normal);
}

#[test]
fn snooze_input_mode_enter_confirms() {
    let mut app = app_with_rows("snooze-confirm", 1);
    app.input_mode = InputMode::SnoozeInput;
    app.snooze_input = "1d".to_string();

    handle_key_event(&mut app, key(KeyCode::Enter));
    assert_eq!(app.input_mode, InputMode::Normal);
}

#[test]
fn snooze_input_mode_ignores_unmapped_keys() {
    let mut app = app_with_rows("snooze-ignore-other", 1);
    app.input_mode = InputMode::SnoozeInput;

    handle_key_event(&mut app, key(KeyCode::Tab));
    assert_eq!(app.input_mode, InputMode::SnoozeInput);
    assert_eq!(app.snooze_input, "");
}

#[test]
fn score_breakdown_mode_navigation_and_dismiss() {
    let mut app = app_with_rows("breakdown-nav", 3);
    app.input_mode = InputMode::ScoreBreakdown;

    handle_key_event(&mut app, key(KeyCode::Char('j')));
    assert_eq!(app.table_state.selected(), Some(1));

    handle_key_event(&mut app, key(KeyCode::Char('b')));
    assert_eq!(app.input_mode, InputMode::Normal);

    app.input_mode = InputMode::ScoreBreakdown;
    handle_key_event(&mut app, key(KeyCode::Esc));
    assert_eq!(app.input_mode, InputMode::Normal);
}

#[test]
fn help_mode_any_key_dismisses() {
    let mut app = test_app("help-dismiss");
    app.input_mode = InputMode::Help;
    handle_key_event(&mut app, key(KeyCode::Char('x')));
    assert_eq!(app.input_mode, InputMode::Normal);
}
