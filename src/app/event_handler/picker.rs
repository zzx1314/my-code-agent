use ratatui::crossterm::event::{self, KeyCode};
use std::sync::Arc;

use crate::app::App;
use crate::core::agent::stream::rebuild_agent;
use crate::core::session::SessionData;
use crate::tools::infra::undo_history::set_session_id;
use crate::core::context::tool_dedup::get_global_tool_dedup;

/// Handle model picker key events. Returns true if the event was consumed.
pub fn handle_model_picker_key(key: event::KeyEvent, app: &mut App) -> bool {
    if !app.show_model_picker {
        return false;
    }

    match key.code {
        KeyCode::Down | KeyCode::Tab => {
            if !app.model_options.is_empty() {
                app.model_selected = (app.model_selected + 1) % app.model_options.len();
            }
            true
        }
        KeyCode::Up | KeyCode::BackTab => {
            if !app.model_options.is_empty() {
                app.model_selected = if app.model_selected == 0 {
                    app.model_options.len() - 1
                } else {
                    app.model_selected - 1
                };
            }
            true
        }
        KeyCode::Enter => {
            if !app.model_options.is_empty() {
                let selected_model = app.model_options[app.model_selected].clone();
                app.config.llm.model = Some(selected_model.clone());
                app.chat_history.push(crate::app::ChatEntry::user(format!("/model {}", selected_model)));

                if let Ok(new_agent) = rebuild_agent(&app.config) {
                    app.agent = Arc::new(new_agent);
                    app.chat_history.push(crate::app::ChatEntry::assistant(format!("Model switched to: {}", selected_model)));
                } else {
                    app.chat_history.push(crate::app::ChatEntry::assistant("Failed to switch model. Please check API key and try again.".to_string()));
                }
            }
            app.show_model_picker = false;
            app.show_banner = false;
            app.auto_scroll = true;
            app.last_reasoning.clear();
            app.streaming_reasoning.clear();
            true
        }
        KeyCode::Esc => {
            app.show_model_picker = false;
            true
        }
        _ => true, // Consume all other keys while picker is open
    }
}

/// Handle provider picker key events. Returns true if the event was consumed.
pub fn handle_provider_picker_key(key: event::KeyEvent, app: &mut App) -> bool {
    if !app.show_provider_picker {
        return false;
    }

    match key.code {
        KeyCode::Down | KeyCode::Tab => {
            if !app.provider_options.is_empty() {
                app.provider_selected = (app.provider_selected + 1) % app.provider_options.len();
            }
            true
        }
        KeyCode::Up | KeyCode::BackTab => {
            if !app.provider_options.is_empty() {
                app.provider_selected = if app.provider_selected == 0 {
                    app.provider_options.len() - 1
                } else {
                    app.provider_selected - 1
                };
            }
            true
        }
        KeyCode::Enter => {
            if !app.provider_options.is_empty() {
                let selected_provider = app.provider_options[app.provider_selected].clone();
                app.config.llm.provider = selected_provider.clone();
                if selected_provider != "custom" {
                    app.config.llm.api_key_env = match selected_provider.as_str() {
                        "deepseek" => "DEEPSEEK_API_KEY".to_string(),
                        "openrouter" => "OPENROUTER_API_KEY".to_string(),
                        _ => String::new(),
                    };
                    app.model_options = crate::app::get_model_options_for_provider(&selected_provider);
                    app.model_selected = 0;
                    app.config.llm.model = app.model_options.first().cloned();
                } else {
                    // Custom provider: preserve config.toml values (model, api_key_env, base_url)
                    let current_model = app.config.llm.model.clone().unwrap_or_default();
                    app.model_options = vec![current_model];
                    app.model_selected = 0;
                }

                app.chat_history.push(crate::app::ChatEntry::user(format!("/connect {}", selected_provider)));

                if let Ok(new_agent) = rebuild_agent(&app.config) {
                    app.agent = Arc::new(new_agent);
                    app.chat_history.push(crate::app::ChatEntry::assistant(format!(
                        "Provider switched to: {} (model: {})",
                        selected_provider,
                        app.config.llm.model.as_deref().unwrap_or("default")
                    )));
                } else {
                    app.chat_history.push(crate::app::ChatEntry::assistant(
                        "Failed to switch provider. Please check API key and try again.".to_string(),
                    ));
                }
            }
            app.show_provider_picker = false;
            app.show_banner = false;
            app.auto_scroll = true;
            app.last_reasoning.clear();
            app.streaming_reasoning.clear();
            true
        }
        KeyCode::Esc => {
            app.show_provider_picker = false;
            true
        }
        _ => true, // Consume all other keys while picker is open
    }
}

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

                match SessionData::load_by_name(&session_name) {
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
                        set_session_id(new_session_id);

                        // Reset tool dedup cache — stale reads from previous session are invalid
                        {
                            let dedup = get_global_tool_dedup();
                            let mut guard = dedup.lock().unwrap();
                            guard.reset();
                        }

                        app.chat_history.push(crate::app::ChatEntry::user(format!("/load {}", session_name)));
                        app.chat_history.push(crate::app::ChatEntry::assistant(format!(
                            "Session '{}' loaded ({} turns, {} tokens)",
                            session_name, selected_session.turns, selected_session.tokens
                        )));
                    }
                    Some(Err(e)) => {
                        app.chat_history.push(crate::app::ChatEntry::user(format!("/load {}", session_name)));
                        app.chat_history.push(crate::app::ChatEntry::assistant(format!("Failed to load session '{}': {}", session_name, e)));
                    }
                    None => {
                        app.chat_history.push(crate::app::ChatEntry::user(format!("/load {}", session_name)));
                        app.chat_history.push(crate::app::ChatEntry::assistant(format!("Session '{}' not found", session_name)));
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
