use tokio::sync::mpsc;

use crate::app::App;

/// Process streaming events (text deltas, tool calls, reasoning)
pub fn process_streaming_events(app: &mut App) {
    if let Some(ref mut rx) = app.streaming_events_rx {
        loop {
            match rx.try_recv() {
                Ok(crate::core::agent::stream_response::StreamEvent::Text(delta)) => {
                    if app.current_tool_call.is_some() {
                        app.streaming_text.push_str("\n\n");
                        app.current_tool_call = None;
                    }
                    app.streaming_text.push_str(&delta);
                }
                Ok(crate::core::agent::stream_response::StreamEvent::ToolCall(name)) => {
                    app.current_tool_call = Some(name);
                }
                Ok(crate::core::agent::stream_response::StreamEvent::ReasoningActive(active)) => {
                    if !active {
                        if !app.streaming_reasoning.is_empty() {
                            app.last_reasoning = app.streaming_reasoning.clone();
                            app.streaming_reasoning.clear();
                        }
                    }
                }
                Ok(crate::core::agent::stream_response::StreamEvent::ReasoningDelta(delta)) => {
                    // Some API providers send FULL accumulated reasoning_content
                    // in each SSE chunk rather than incremental deltas.
                    if delta.starts_with(&app.streaming_reasoning) {
                        app.streaming_reasoning = delta;
                    } else {
                        app.streaming_reasoning.push_str(&delta);
                    }
                    app.current_tool_call = None;
                }
                Err(mpsc::error::TryRecvError::Empty) => break,
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    app.streaming_events_rx = None;
                    break;
                }
            }
        }
    }
}
