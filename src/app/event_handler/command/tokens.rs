use crate::app::App;

pub(super) fn handle(app: &mut App) -> bool {
    app.chat_history
        .push(("user".to_string(), "/tokens".to_string()));
    let mut report = app.token_usage.format_session_report();
    // Append session-wide cache metrics
    let cache_report = crate::core::context_cache::global_cache().format_session_report();
    report.extend(cache_report);
    let token_info = report.join("\n").trim().to_string();
    app.chat_history.push(("assistant".to_string(), token_info));
    app.show_banner = false;
    app.auto_scroll = true;
    true
}
