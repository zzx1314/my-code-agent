mod completion;
mod input;
mod mouse;
pub use mouse::handle_mouse_event;
mod paste;

use crate::app::App;
use crate::app::event_handler::picker::{
    handle_model_picker_key, handle_provider_picker_key, handle_session_picker_key,
};
use crate::app::terminal;
use crate::core::context::context_manager::ContextManager;
use completion::{
    apply_completion, get_cursor_position, hide_completion, trigger_completion,
    update_completion_query,
};
use input::handle_enter_key;
use input::{history_down, history_up};
pub use paste::handle_paste_event;
use ratatui::crossterm::event::{self, KeyCode, KeyModifiers};
/// Handle key events
pub fn handle_key_event(key: event::KeyEvent, app: &mut App, context_manager: &mut ContextManager) {
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
            KeyCode::Char(_c) => {
                // When completion is already active, treat all characters as
                // part of the query (e.g., '/' in '@src/main.rs' should not
                // switch to command completion)
                app.input.input(key);
                update_completion_query(app);
                return;
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
        (KeyCode::Char('e'), modifiers) if modifiers.contains(KeyModifiers::ALT) => {
            // Alt+E: toggle the last collapsible section (closest to bottom)
            if let Some(last_idx) = app.collapsed_toggles.len().checked_sub(1) {
                let section_id = app.collapsed_toggles[last_idx].1.clone();
                if app.collapsed_sections.contains(&section_id) {
                    app.collapsed_sections.remove(&section_id);
                } else {
                    app.collapsed_sections.insert(section_id);
                }
                app.auto_scroll = false;
            }
        }
        (KeyCode::Char('s'), modifiers) if modifiers.contains(KeyModifiers::ALT) => {
            // Alt+S: toggle selection mode
            app.selection_mode = !app.selection_mode;
            if app.selection_mode {
                terminal::disable_mouse_tracking();
            } else {
                terminal::enable_mouse_tracking();
            }
        }
        (KeyCode::Esc, _) => {
            if app.show_completion {
                hide_completion(app);
            } else if app.is_streaming {
                let _ = app.interrupt_tx.send(());
                app.response_rx = None;
                app.streaming_events_rx = None;
                app.init_rx = None;

                // Save partial streaming content to chat history before clearing
                let mut display = std::mem::take(&mut app.streaming_text);

                // Merge all reasoning segments
                let mut reasoning = std::mem::take(&mut app.streaming_reasoning);
                for seg in app.completed_pre_text_segments.drain(..) {
                    if !seg.trim_end().is_empty() {
                        if !reasoning.is_empty() {
                            reasoning.push('\n');
                        }
                        reasoning.push_str(seg.trim_end());
                    }
                }
                for seg in app.completed_post_text_segments.drain(..) {
                    if !seg.trim_end().is_empty() {
                        if !reasoning.is_empty() {
                            reasoning.push('\n');
                        }
                        reasoning.push_str(seg.trim_end());
                    }
                }
                if !app.post_text_reasoning.is_empty() {
                    let trimmed = app.post_text_reasoning.trim_end();
                    if !trimmed.is_empty() {
                        if !reasoning.is_empty() {
                            reasoning.push('\n');
                        }
                        reasoning.push_str(trimmed);
                    }
                    app.post_text_reasoning.clear();
                }
                display.push_str(" ⚡*interrupted*");
                if reasoning.is_empty() {
                    app.chat_history
                        .push(crate::app::ChatEntry::assistant(display));
                } else {
                    app.chat_history
                        .push(crate::app::ChatEntry::assistant_with_reasoning(
                            display, &reasoning,
                        ));
                }
                app.show_inline_reasoning = !reasoning.is_empty();
                app.auto_scroll = true;

                app.is_streaming = false;
                app.text_segment_boundaries.clear();
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
        (KeyCode::Up, _) if app.show_completion => {
            if !app.completion_items.is_empty() {
                app.completion_selected = if app.completion_selected == 0 {
                    app.completion_items.len() - 1
                } else {
                    app.completion_selected - 1
                };
            }
        }
        (KeyCode::Up, modifiers) if modifiers.is_empty() => {
            if !app.show_completion {
                // ↑ key → history navigation (only when !show_completion)
                if app.history_index.is_some() || app.input.is_empty() {
                    history_up(app);
                } else {
                    app.input.input(key);
                }
            }
        }
        (KeyCode::Down, _) if app.show_completion => {
            if !app.completion_items.is_empty() {
                app.completion_selected =
                    (app.completion_selected + 1) % app.completion_items.len();
            }
        }
        (KeyCode::Down, modifiers) if modifiers.is_empty() => {
            if !app.show_completion {
                // ↓ key → history navigation (only when !show_completion)
                if app.history_index.is_some() || app.input.is_empty() {
                    history_down(app);
                } else {
                    app.input.input(key);
                }
            }
        }
        (KeyCode::Char(c), _) => {
            app.history_index = None; // Exit history browsing on any typed character
            if c == '/' || c == '@' {
                app.input.input(key);
                if c == '/' {
                    let cursor = app.input.cursor();
                    if cursor.1 == 1 {
                        // '/' at beginning of input triggers command completion
                        trigger_completion(app, '/');
                    } else {
                        // Check if '/' continues an '@' file path (e.g., '@src/')
                        let lines: Vec<String> =
                            app.input.lines().iter().map(|s| s.to_string()).collect();
                        if cursor.0 < lines.len() {
                            let line = &lines[cursor.0];
                            let byte_pos = line
                                .char_indices()
                                .nth(cursor.1)
                                .map(|(i, _)| i)
                                .unwrap_or(line.len());
                            let before_cursor = &line[..byte_pos];
                            if let Some(at_pos) = before_cursor.rfind('@') {
                                if !before_cursor[at_pos + 1..].contains(' ') {
                                    trigger_completion(app, '@');
                                }
                            }
                        }
                    }
                } else {
                    trigger_completion(app, '@');
                }
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
