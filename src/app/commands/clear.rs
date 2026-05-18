use crate::app::App;

pub fn handle(app: &mut App) -> bool {
    app.chat_history.clear();
    app.current_response.clear();
    app.last_reasoning.clear();
    app.streaming_reasoning.clear();
    app.streaming_text.clear();
    app.token_usage.reset();
    app.current_tool_call = None;
    app.streaming_status.clear();
    app.status_messages.clear();
    app.turn_usage_line = None;
    app.streaming_events_rx = None;
    app.show_inline_reasoning = false;
    app.rendered_cache.clear();
    app.git_diff_cache.clear();
    true
}
