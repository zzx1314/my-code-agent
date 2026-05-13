use ratatui::crossterm::event::{self, KeyCode};
use std::sync::Arc;

use crate::app::App;

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

                if let Ok(new_agent) =
                    crate::core::agent::stream::rebuild_agent(&app.config)
                {
                    app.agent = Arc::new(new_agent);
                    app.chat_history.push(crate::app::ChatEntry::assistant(format!("Model switched to: {}", selected_model),));
                } else {
                    app.chat_history.push(crate::app::ChatEntry::assistant("Failed to switch model. Please check API key and try again.".to_string(),));
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
