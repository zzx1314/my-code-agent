use ratatui::crossterm::event::{self, KeyCode};
use std::sync::Arc;

use crate::app::App;

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

                app.chat_history.push(crate::app::ChatEntry::user(format!("/connect {}", selected_provider),));

                if let Ok(new_agent) =
                    crate::app::event_handler::stream::rebuild_agent(&app.config)
                {
                    app.agent = Arc::new(new_agent);
                    app.chat_history.push(crate::app::ChatEntry::assistant(format!(
                        "Provider switched to: {} (model: {})",
                        selected_provider,
                        app.config.llm.model.as_deref().unwrap_or("default")
                    ),));
                } else {
                    app.chat_history.push(crate::app::ChatEntry::assistant("Failed to switch provider. Please check API key and try again."
                        .to_string(),));
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
