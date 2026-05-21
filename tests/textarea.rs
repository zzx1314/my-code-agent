//! Integration tests for [`TextArea`] — exercises the public API only.
//!
//! Tests that access private methods / fields remain inline in
//! `src/ui/textarea.rs` as `#[cfg(test)]` unit tests.

use my_code_agent::ui::textarea::TextArea;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::{Color, Style};
use ratatui::widgets::Block;

// ── helpers ──────────────────────────────────────────────────────────────────

fn ta(text: &str) -> TextArea {
    TextArea::from(text)
}

// ── Construction ─────────────────────────────────────────────────────────────

#[test]
fn new_is_empty() {
    let t = TextArea::new();
    assert!(t.is_empty());
    assert_eq!(t.text(), "");
    assert_eq!(t.cursor(), (0, 0));
}

#[test]
fn from_str_sets_text() {
    let t = ta("hello");
    assert_eq!(t.text(), "hello");
    assert_eq!(t.cursor(), (0, 5));
}

// ── Text manipulation (public API) ───────────────────────────────────────────

#[test]
fn insert_str_adds_at_cursor() {
    let mut t = ta("helo");
    t.set_cursor(2);
    t.insert_str("ll");
    assert_eq!(t.text(), "helllo");
}

#[test]
fn insert_str_at_position() {
    let mut t = ta("ab");
    t.insert_str_at(1, "XY");
    assert_eq!(t.text(), "aXYb");
}

#[test]
fn lines_splits_by_newline() {
    let t = ta("abc\ndef\nghi");
    let lines = t.lines();
    assert_eq!(lines, vec!["abc", "def", "ghi"]);
}

// ── Cursor ───────────────────────────────────────────────────────────────────

#[test]
fn cursor_conversion() {
    let mut t = ta("hello\nworld");
    // Cursor at byte 0 → (0, 0)
    t.set_cursor(0);
    assert_eq!(t.cursor(), (0, 0));
    // Byte 6 = first char of second line = 'w'
    t.set_cursor(6);
    assert_eq!(t.cursor(), (1, 0));
    // Byte 7 = 'o' on second line
    t.set_cursor(7);
    assert_eq!(t.cursor(), (1, 1));
}

#[test]
fn move_cursor_jump() {
    let mut t = ta("hello\nworld");
    t.move_cursor(1, 2); // row 1, col 2 → "rl"
    assert_eq!(t.text()[t.cursor_byte_pos()..].chars().next(), Some('r'));
}

// ── Input routing ────────────────────────────────────────────────────────────

#[test]
fn cursor_movement_by_keys() {
    let mut t = ta("ab");
    t.set_cursor(0);
    let event = |code| KeyEvent::new(code, KeyModifiers::NONE);

    t.input(event(KeyCode::Right));
    assert_eq!(t.cursor(), (0, 1));
    t.input(event(KeyCode::Right));
    assert_eq!(t.cursor(), (0, 2));
    t.input(event(KeyCode::Left));
    assert_eq!(t.cursor(), (0, 1));
}

#[test]
fn typing_characters() {
    let mut t = TextArea::new();
    t.input(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
    assert_eq!(t.text(), "a");
    t.input(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE));
    assert_eq!(t.text(), "ab");
}

#[test]
fn newline_insertion() {
    let mut t = ta("abc");
    t.set_cursor(1);
    t.insert_str("\n");
    assert_eq!(t.text(), "a\nbc");
    let lines = t.lines();
    assert_eq!(lines, vec!["a", "bc"]);
}

// ── Styling ──────────────────────────────────────────────────────────────────

#[test]
fn set_block_and_style() {
    let mut t = TextArea::new();
    let block = Block::default()
        .title(" Test ")
        .borders(ratatui::widgets::Borders::ALL);
    t.set_block(block);
    t.set_cursor_line_style(Style::default().bg(Color::Rgb(20, 40, 50)));
    // Just verify no panic
    assert_eq!(t.text(), "");
}

// ── Sizing ───────────────────────────────────────────────────────────────────

#[test]
fn desired_height_grows_with_content() {
    let t = ta("hello\nworld\nfoo\nbar\nbaz");
    let h = t.desired_height(100);
    assert_eq!(h, 5); // 4 newlines = 5 logical lines
    let h_narrow = t.desired_height(3);
    assert!(h_narrow > h);
}
