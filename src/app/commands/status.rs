use crate::app::App;

/// Displays the current application status (streaming/idle, model, provider,
/// message count, and token usage) by pushing an assistant message into the
/// chat history.
pub fn handle(app: &mut App) -> bool {
    // Determine the current streaming state.
    let status = if app.is_streaming { "🔄 Streaming" } else { "⏸️ Idle" };

    // Retrieve the configured model name (fall back to "not set").
    let model = app
        .config
        .llm
        .model
        .as_deref()
        .unwrap_or("not set");

    // Read the provider name.
    let provider = &app.config.llm.provider;

    // Gather aggregate statistics.
    let chat_count = app.chat_history.len();
    let token_count = app.token_usage.total_tokens();

    // Format and append a status report as an assistant message.
    app.chat_history.push(crate::app::ChatEntry::assistant(format!(
        "**Status**\n\
         - Status: {}\n\
         - Provider: {}\n\
         - Model: {}\n\
         - Messages: {}\n\
         - Tokens: {}",
        status, provider, model, chat_count, token_count,
    )));

    // Suppress the banner on the next render (status was already shown inline).
    app.show_banner = false;

    true
}
