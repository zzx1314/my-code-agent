mod completion;
mod input;
mod mouse;
mod paste;

use ratatui::crossterm::event::{self, KeyCode, KeyModifiers};

use crate::app::App;
use crate::core::context::context_manager::ContextManager;
use completion::{
    apply_completion, get_cursor_position, hide_completion, trigger_completion,
    update_completion_query,
};
use input::handle_enter_key;
use input::{history_down, history_up};
pub use mouse::handle_mouse_event;
pub use paste::handle_paste_event;
use crate::app::event_handler::picker::{handle_model_picker_key, handle_provider_picker_key, handle_session_picker_key};

/// Handle key events
pub fn handle_key_event(key: event::KeyEvent, app: &mut App, context_manager: &mut ContextManager) {
    // If the confirmation dialog is showing, handle confirmation-related keys first
    if app.pending_confirmation.is_some() {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                if let Some(pending) = app.pending_confirmation.take() {
                    let _ = pending.response_tx.send(true);
                }
                return;
            }
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                if let Some(pending) = app.pending_confirmation.take() {
                    let _ = pending.response_tx.send(false);
                }
                return;
            }
            _ => return,
        }
    }

    // Delegate to picker handlers
    if handle_provider_picker_key(key, app)
        || handle_session_picker_key(key, app)
        || handle_model_picker_key(key, app)
    {
        return;
    }

    // If the completion menu is showing, handle completion-related keys first
    if app.show_completion {
        match key.code {
            KeyCode::Down | KeyCode::Tab => {
                if !app.completion_items.is_empty() {
                    app.completion_selected =
                        (app.completion_selected + 1) % app.completion_items.len();
                }
                return;
            }
            KeyCode::Up | KeyCode::BackTab => {
                if !app.completion_items.is_empty() {
                    app.completion_selected = if app.completion_selected == 0 {
                        app.completion_items.len() - 1
                    } else {
                        app.completion_selected - 1
                    };
                }
                return;
            }
            KeyCode::Enter => {
                let is_command_completion = app.completion_type == Some('/');
                apply_completion(app);
                if is_command_completion {
                    handle_enter_key(app, context_manager);
                }
                return;
            }
            KeyCode::Esc => {
                hide_completion(app);
                return;
            }
            KeyCode::Char(c) => {
                if c == '/' && app.completion_type != Some('/') {
                    hide_completion(app);
                    trigger_completion(app, '/');
                    return;
                } else if c == '@' && app.completion_type != Some('@') {
                    hide_completion(app);
                    trigger_completion(app, '@');
                    return;
                }
            }
            KeyCode::Backspace => {}
            _ => {}
        }
    }

    match (key.code, key.modifiers) {
        (KeyCode::Char('c'), modifiers) if modifiers.contains(KeyModifiers::CONTROL) => {
            if !app.is_streaming {
                app.should_exit = true;
            }
        }
        (KeyCode::Char('r'), modifiers) if modifiers.contains(KeyModifiers::CONTROL) => {
            app.show_reasoning = !app.show_reasoning;
        }
        (KeyCode::Esc, _) => {
            if app.show_completion {
                hide_completion(app);
            } else if app.is_streaming {
                let _ = app.interrupt_tx.send(());
                app.response_rx = None;
                app.streaming_events_rx = None;
                app.init_rx = None;
                app.is_streaming = false;
                app.streaming_text.clear();
                app.streaming_reasoning.clear();
                app.current_tool_call = None;
                app.status_messages.clear();
            } else {
                app.should_exit = true;
            }
        }
        (KeyCode::Enter, modifiers) => {
            if modifiers.contains(KeyModifiers::ALT) {
                app.input.input(key);
            } else {
                if app.show_completion {
                    let is_command_completion = app.completion_type == Some('/');
                    apply_completion(app);
                    if is_command_completion {
                        handle_enter_key(app, context_manager);
                    }
                } else {
                    handle_enter_key(app, context_manager);
                }
            }
        }
        (KeyCode::PageUp, _) => {
            app.scroll = app.scroll.saturating_sub(3);
            app.auto_scroll = false;
        }
        (KeyCode::PageDown, _) => {
            let max_scroll = app.total_lines.saturating_sub(app.chat_area_height);
            app.scroll = (app.scroll + 3).min(max_scroll);
            app.auto_scroll = false;
        }
        (KeyCode::Up, modifiers) if modifiers.is_empty() => {
            if app.show_completion {
                if !app.completion_items.is_empty() {
                    app.completion_selected = if app.completion_selected == 0 {
                        app.completion_items.len() - 1
                    } else {
                        app.completion_selected - 1
                    };
                }
            } else {
                history_up(app);
            }
        }
        (KeyCode::Down, modifiers) if modifiers.is_empty() => {
            if app.show_completion {
                if !app.completion_items.is_empty() {
                    app.completion_selected =
                        (app.completion_selected + 1) % app.completion_items.len();
                }
            } else {
                history_down(app);
            }
        }
        (KeyCode::Char(c), _) => {
            app.history_index = None; // Exit history browsing on any typed character
            if c == '/' || c == '@' {
                app.input.input(key);
                trigger_completion(app, c);
            } else {
                app.input.input(key);
                if app.show_completion {
                    update_completion_query(app);
                }
            }
        }
        (KeyCode::Backspace, _) => {
            app.history_index = None; // Exit history browsing on backspace
            app.input.input(key);
            if app.show_completion {
                let cursor_pos = get_cursor_position(app);
                if cursor_pos == 0 || (cursor_pos <= app.completion_trigger_pos) {
                    hide_completion(app);
                } else {
                    update_completion_query(app);
                }
            }
        }
        _ => {
            app.input.input(key);
        }
    }
}
