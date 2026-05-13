use crate::app::App;

pub(super) fn handle(app: &mut App) -> bool {
    app.chat_history.push(crate::app::ChatEntry::user("/status".to_string()));
    let status = format!(
        "Session enabled: {}\nModel: {}\nProvider: {}\nTotal tokens used: {}",
        app.config.session.enabled,
        app.config.llm.model.as_deref().unwrap_or("default"),
        app.config.llm.provider,
        app.token_usage.total_tokens()
    );
    app.chat_history.push(crate::app::ChatEntry::assistant(status));
    app.show_banner = false;
    app.auto_scroll = true;
    true
}
