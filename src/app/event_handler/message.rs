use tui_textarea::TextArea;

use crate::app::App;
use crate::core::context_manager::ContextManager;

use super::streaming::{reset_streaming_state, spawn_llm_stream};

/// Send a message to the LLM (extracted for reuse by message queue)
pub fn send_message_to_llm(
    app: &mut App,
    context_manager: &mut ContextManager,
    input_text: String,
) {
    app.show_banner = false; // Hide startup banner
    app.chat_history
        .push(("user".to_string(), input_text.clone()));
    app.input = {
        let mut ta = TextArea::default();
        ta.set_block(
            ratatui::widgets::Block::default()
                .borders(ratatui::widgets::Borders::ALL)
                .title(" Input (Enter to send, Alt+Enter for newline, Esc: interrupt/exit) "),
        );
        ta.set_cursor_line_style(ratatui::style::Style::default());
        ta
    };
    reset_streaming_state(app);
    spawn_llm_stream(app, context_manager, &input_text);
}

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

/// Handle mouse events
pub fn handle_mouse_event(mouse: ratatui::crossterm::event::MouseEvent, app: &mut App) {
    use ratatui::crossterm::event::MouseEventKind;

    match mouse.kind {
        MouseEventKind::ScrollUp => {
            app.scroll = app.scroll.saturating_sub(3);
            app.auto_scroll = false;
        }
        MouseEventKind::ScrollDown => {
            let max_scroll = app.total_lines.saturating_sub(app.chat_area_height);
            app.scroll = (app.scroll + 3).min(max_scroll);
            // Re-enable auto_scroll when scrolled to the bottom
            if app.scroll >= max_scroll {
                app.auto_scroll = true;
            }
        }
        _ => {} // Ignore other mouse events without affecting text selection
    }
}
