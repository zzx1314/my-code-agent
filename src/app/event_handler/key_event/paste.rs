use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::App;

pub fn handle_paste_event(text: &str, app: &mut App) {
    for ch in text.chars() {
        if ch == '\n' || ch == '\r' {
            // Insert newline directly via the textarea rather than faking a key event.
            app.input.insert_str("\n");
        } else {
            let key = KeyEvent::new(
                KeyCode::Char(ch),
                KeyModifiers::NONE,
            );
            app.input.input(key);
        }
    }
}
