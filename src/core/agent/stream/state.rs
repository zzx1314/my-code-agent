use crate::app::App;

/// Reset the streaming state to prepare for a new LLM response
pub fn reset_streaming_state(app: &mut App) {
    app.is_streaming = true;
    app.auto_scroll = true;
    app.reasoning_auto_scroll = true;
    app.reasoning_scroll = 0;
    app.streaming_text.clear();
    app.streaming_reasoning.clear();
    app.last_reasoning.clear();
    app.current_tool_call = None;
    app.current_response.clear();
    app.streaming_status.clear();
    app.status_messages.clear();
    app.turn_usage_line = None;
}

/// Clean up streaming state on disconnect/error
pub fn cleanup_stream_state(app: &mut App) {
    app.is_streaming = false;
    app.streaming_text.clear();
    app.streaming_reasoning.clear();
    app.current_tool_call = None;
    app.streaming_events_rx = None;
    app.streaming_status.clear();
    app.auto_scroll = true;
}
