use crate::app::App;

pub(super) fn handle(app: &mut App) -> bool {
    app.chat_history.clear();
    app.token_usage = crate::core::token_usage::TokenUsage::with_config(&app.config);
    app.show_banner = true;
    app.auto_scroll = true;
    app.scroll = 0;
    app.last_reasoning.clear();
    app.streaming_reasoning.clear();
    app.streaming_text.clear();
    app.current_response.clear();
    app.show_inline_reasoning = false;
    app.current_tool_call = None;
    app.status_messages.clear();
    app.turn_usage_line = None;
    app.streaming_events_rx = None;
    // Delete session file
    if app.config.session.enabled {
        if let Some(save_file) = &app.config.session.save_file {
            let _ = std::fs::remove_file(save_file);
        }
    }
    // Clear undo history for current session
    if let Err(e) = crate::tools::undo_history::clear_current_session_entries() {
        tracing::warn!(error = %e, "Failed to clear undo history on /clear");
    }
    // Reset tool dedup cache — no stale reads from previous session
    {
        let dedup = crate::core::tool_dedup::get_global_tool_dedup();
        let mut guard = dedup.lock().unwrap();
        guard.reset();
    }
    // Generate a new session ID so the cleared session's history is separate
    let new_session_id = format!(
        "session_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    crate::tools::undo_history::set_session_id(new_session_id);
    true
}
