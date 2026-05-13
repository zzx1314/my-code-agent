use crate::app::App;

pub fn handle_paste_event(text: &str, app: &mut App) {
    for ch in text.chars() {
        if ch == '\n' || ch == '\r' {
            let key = ratatui::crossterm::event::KeyEvent::new(
                ratatui::crossterm::event::KeyCode::Enter,
                ratatui::crossterm::event::KeyModifiers::ALT,
            );
            app.input.input(key);
        } else {
            let key = ratatui::crossterm::event::KeyEvent::new(
                ratatui::crossterm::event::KeyCode::Char(ch),
                ratatui::crossterm::event::KeyModifiers::NONE,
            );
            app.input.input(key);
        }
    }
}
