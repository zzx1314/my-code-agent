use crate::app::App;

pub fn handle(app: &mut App) -> bool {
    app.chat_history.push(crate::app::ChatEntry::user("/tokens".to_string()));
    let mut report = app.token_usage.format_session_report();
    // Append session-wide cache metrics
    let cache_report = crate::core::context::context_cache::global_cache().format_session_report();
    report.extend(cache_report);
    let token_info = report.join("\n").trim().to_string();
    app.chat_history.push(crate::app::ChatEntry::assistant(token_info));
    app.show_banner = false;
    app.auto_scroll = true;
    true
}
