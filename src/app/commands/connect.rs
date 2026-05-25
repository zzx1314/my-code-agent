use std::sync::Arc;

use crate::app::App;
use crate::core::agent::stream::rebuild_agent;

/// Handle the "connect" command — either open the provider picker
/// (when invoked as bare `/connect`) or switch directly to a named provider
/// (when invoked as `/connect <provider>`).
///
/// Supported providers: deepseek, openrouter, ollama, custom
///
/// # Arguments
/// - `app`: Mutable reference to the application state.
/// - `input`: The raw command input string (e.g. `/connect`, `/connect ollama`).
///
/// # Returns
/// Always returns `true`, signaling that the command was handled.
pub fn handle(app: &mut App, input: &str) -> bool {
    let parts: Vec<&str> = input.trim().splitn(2, ' ').collect();

    if parts.len() == 1 {
        // Just "/connect" — open the provider picker
        app.show_provider_picker = true;
        if let Some(pos) = app
            .provider_options
            .iter()
            .position(|p| p == &app.config.llm.provider)
        {
            app.provider_selected = pos;
        }
        return true;
    }

    // "/connect <provider>" — direct switch
    let provider_name = parts[1].trim().to_lowercase();

    // Validate the provider name
    if !app.provider_options.contains(&provider_name) {
        app.chat_history
            .push(crate::app::ChatEntry::assistant(format!(
                "Unknown provider: '{}'. Available providers: {}",
                provider_name,
                app.provider_options.join(", ")
            )));
        return true;
    }

    app.chat_history.push(crate::app::ChatEntry::user(format!(
        "/connect {}",
        provider_name
    )));

    // Apply the provider configuration (shared with the provider picker)
    crate::app::apply_provider_config(app, &provider_name);

    let succeeded = match rebuild_agent(&app.config, &app.skill_manager) {
        Ok(new_agent) => {
            app.agent = Arc::new(new_agent);
            true
        }
        Err(_) => false,
    };

    if succeeded {
        app.chat_history
            .push(crate::app::ChatEntry::assistant(format!(
                "Provider switched to: {} (model: {})",
                provider_name,
                app.config.llm.model.as_deref().unwrap_or("default")
            )));
    } else {
        app.chat_history.push(crate::app::ChatEntry::assistant(
            "Failed to switch provider. Please check API key and try again.".to_string(),
        ));
    }

    app.show_banner = false;
    app.auto_scroll = true;
    app.last_reasoning.clear();
    app.streaming_reasoning.clear();

    true
}
