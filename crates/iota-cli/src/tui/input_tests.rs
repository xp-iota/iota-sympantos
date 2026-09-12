use crate::tui::input::*;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn modified(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent::new(code, modifiers)
}

fn type_text(composer: &mut Composer, text: &str) {
    for ch in text.chars() {
        assert_eq!(
            composer.handle_key(key(KeyCode::Char(ch))),
            ComposerAction::Changed
        );
    }
}

#[test]
fn edits_unicode_by_grapheme_cluster() {
    let mut composer = Composer::new();
    type_text(&mut composer, "aé猫");

    assert_eq!(composer.cursor, 3);
    assert_eq!(
        composer.handle_key(key(KeyCode::Left)),
        ComposerAction::Changed
    );
    assert_eq!(
        composer.handle_key(key(KeyCode::Backspace)),
        ComposerAction::Changed
    );

    assert_eq!(composer.text, "a猫");
    assert_eq!(composer.cursor, 1);
}

#[test]
fn shift_enter_inserts_newline_and_display_lines_tracks_cursor() {
    let mut composer = Composer::new();
    type_text(&mut composer, "hello");
    assert_eq!(
        composer.handle_key(modified(KeyCode::Enter, KeyModifiers::SHIFT)),
        ComposerAction::Changed
    );
    type_text(&mut composer, "world");

    let (lines, row, col) = composer.display_lines();
    assert_eq!(lines, vec!["hello".to_string(), "world".to_string()]);
    assert_eq!((row, col), (1, 5));
}

#[test]
fn ctrl_k_and_ctrl_y_roundtrip_kill_buffer() {
    let mut composer = Composer::new();
    type_text(&mut composer, "alpha beta");
    for _ in 0..5 {
        composer.handle_key(key(KeyCode::Left));
    }

    assert_eq!(
        composer.handle_key(modified(KeyCode::Char('k'), KeyModifiers::CONTROL)),
        ComposerAction::Changed
    );
    assert_eq!(composer.text, "alpha");
    assert_eq!(composer.cursor, 5);

    assert_eq!(
        composer.handle_key(modified(KeyCode::Char('y'), KeyModifiers::CONTROL)),
        ComposerAction::Changed
    );
    assert_eq!(composer.text, "alpha beta");
    assert_eq!(composer.cursor, 10);
}

#[test]
fn history_recall_restores_saved_draft_on_down() {
    let mut composer = Composer::new();
    type_text(&mut composer, "first");
    assert_eq!(composer.take_submit(), "first");
    type_text(&mut composer, "draft");

    assert_eq!(
        composer.handle_key(key(KeyCode::Up)),
        ComposerAction::Changed
    );
    assert_eq!(composer.text, "first");
    assert_eq!(
        composer.handle_key(key(KeyCode::Down)),
        ComposerAction::Changed
    );
    assert_eq!(composer.text, "draft");
}

#[test]
fn ctrl_r_search_accepts_matching_history_entry() {
    let mut composer = Composer::new();
    type_text(&mut composer, "build project");
    composer.take_submit();
    type_text(&mut composer, "run tests");
    composer.take_submit();

    assert_eq!(
        composer.handle_key(modified(KeyCode::Char('r'), KeyModifiers::CONTROL)),
        ComposerAction::SearchMode
    );
    assert!(composer.is_searching());
    type_text(&mut composer, "build");
    assert_eq!(composer.search_query(), Some("build"));
    assert_eq!(composer.text, "build project");

    assert_eq!(
        composer.handle_key(key(KeyCode::Enter)),
        ComposerAction::Changed
    );
    assert!(!composer.is_searching());
    assert_eq!(composer.text, "build project");
}

#[test]
fn take_submit_trims_and_deduplicates_tail_history() {
    let mut composer = Composer::new();
    type_text(&mut composer, "  ping  ");
    assert_eq!(composer.take_submit(), "ping");
    type_text(&mut composer, "ping");
    assert_eq!(composer.take_submit(), "ping");

    assert_eq!(
        composer.handle_key(key(KeyCode::Up)),
        ComposerAction::Changed
    );
    assert_eq!(composer.text, "ping");
    assert_eq!(
        composer.handle_key(key(KeyCode::Up)),
        ComposerAction::Changed
    );
    assert_eq!(composer.text, "ping");
}
