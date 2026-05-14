use tokio::sync::mpsc;

use crate::app::App;

/// Process streaming events (text deltas, tool calls, reasoning)
pub fn process_streaming_events(app: &mut App) {
    if let Some(ref mut rx) = app.streaming_events_rx {
        loop {
            match rx.try_recv() {
                Ok(crate::core::agent::stream_response::StreamEvent::Text(delta)) => {
                    if app.current_tool_call.is_some() {
                        app.streaming_text.push_str("\n");
                        app.current_tool_call = None;
                    }
                    // Clear tool result when new text arrives — model is responding
                    app.streaming_tool_result = None;
                    app.streaming_text.push_str(&delta);
                }
                Ok(crate::core::agent::stream_response::StreamEvent::ToolCall { name, arguments }) => {
                    // Don't clear or add newline here — ToolCall events for the same
                    // tool may arrive in multiple chunks with progressively more complete
                    // arguments. Just update the current tool call info.
                    app.current_tool_call = Some(crate::app::CurrentToolCall { name, arguments });
                    // Clear previous tool result — a new tool call is starting
                    app.streaming_tool_result = None;
                }
                Ok(crate::core::agent::stream_response::StreamEvent::ToolResult { name, content }) => {
                    // Store the completed tool result for display during streaming
                    app.current_tool_call = None;
                    app.streaming_tool_result = Some((name, content));
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
                    // NOTE: Do NOT clear current_tool_call here.
                    // Reasoning deltas from a new SSE turn (after tool execution) would
                    // clear the tool-call flag, preventing the subsequent Text event from
                    // inserting the `\n\n` paragraph separator between turns.
                    // This specifically affects reasoning models (e.g. DeepSeek Reasoner)
                    // where a tool call is followed by reasoning + text in the next turn.
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
