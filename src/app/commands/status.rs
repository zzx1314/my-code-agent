use crate::app::App;

pub fn handle(app: &mut App) -> bool {
    let status = if app.is_streaming { "🔄 Streaming" } else { "⏸️ Idle" };
    let model = app
        .config
        .llm
        .model
        .as_deref()
        .unwrap_or("not set");
    let provider = &app.config.llm.provider;
    let chat_count = app.chat_history.len();
    let token_count = app.token_usage.total_tokens();
    app.chat_history.push(crate::app::ChatEntry::assistant(format!(
        "**Status**\n\
         - Status: {}\n\
         - Provider: {}\n\
         - Model: {}\n\
         - Messages: {}\n\
         - Tokens: {}",
        status, provider, model, chat_count, token_count,
    )));
    app.show_banner = false;
    true
}
