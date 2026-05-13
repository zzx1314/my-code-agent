use ratatui::crossterm::event::{self, KeyCode};

use crate::app::App;

/// Handle session picker key events. Returns true if the event was consumed.
pub fn handle_session_picker_key(key: event::KeyEvent, app: &mut App) -> bool {
    if !app.show_session_picker {
        return false;
    }

    match key.code {
        KeyCode::Down | KeyCode::Tab => {
            if !app.session_options.is_empty() {
                app.session_selected = (app.session_selected + 1) % app.session_options.len();
            }
            true
        }
        KeyCode::Up | KeyCode::BackTab => {
            if !app.session_options.is_empty() {
                app.session_selected = if app.session_selected == 0 {
                    app.session_options.len() - 1
                } else {
                    app.session_selected - 1
                };
            }
            true
        }
        KeyCode::Enter => {
            if !app.session_options.is_empty() {
                let selected_session = &app.session_options[app.session_selected];
                let session_name = selected_session.name.clone();

                match crate::core::session::SessionData::load_by_name(&session_name) {
                    Some(Ok(session_data)) => {
                        app.chat_history = session_data
                            .chat_history
                            .into_iter()
                            .map(crate::app::ChatEntry::from_message)
                            .collect();
                        app.token_usage = session_data.token_usage;
                        app.last_reasoning = session_data.last_reasoning;

                        let new_session_id = format!(
                            "session_{}",
                            std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_nanos()
                        );
                        crate::tools::undo_history::set_session_id(new_session_id);

                        // Reset tool dedup cache — stale reads from previous session are invalid
                        {
                            let dedup = crate::core::context::tool_dedup::get_global_tool_dedup();
                            let mut guard = dedup.lock().unwrap();
                            guard.reset();
                        }

                        app.chat_history.push(crate::app::ChatEntry::user(format!("/load {}", session_name)));
                        app.chat_history.push(crate::app::ChatEntry::assistant(format!(
                            "Session '{}' loaded ({} turns, {} tokens)",
                            session_name, selected_session.turns, selected_session.tokens
                        ),));
                    }
                    Some(Err(e)) => {
                        app.chat_history.push(crate::app::ChatEntry::user(format!("/load {}", session_name)));
                        app.chat_history.push(crate::app::ChatEntry::assistant(format!("Failed to load session '{}': {}", session_name, e),));
                    }
                    None => {
                        app.chat_history.push(crate::app::ChatEntry::user(format!("/load {}", session_name)));
                        app.chat_history.push(crate::app::ChatEntry::assistant(format!("Session '{}' not found", session_name),));
                    }
                }
            }
            app.show_session_picker = false;
            app.show_banner = false;
            app.auto_scroll = true;
            true
        }
        KeyCode::Esc => {
            app.show_session_picker = false;
            true
        }
        _ => true, // Consume all other keys while picker is open
    }
}
