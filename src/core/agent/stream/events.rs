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
                    // Clear tool result and status when new text arrives — model is responding
                    app.streaming_tool_result = None;
                    app.streaming_status.clear();
                    app.streaming_text.push_str(&delta);
                }
                Ok(crate::core::agent::stream_response::StreamEvent::ToolCall { name, arguments }) => {
                    // Don't clear or add newline here — ToolCall events for the same
                    // tool may arrive in multiple chunks with progressively more complete
                    // arguments. Just update the current tool call info.
                    app.current_tool_call = Some(crate::app::CurrentToolCall { name, arguments });
                    // Clear previous tool result and status — a new tool call is starting
                    app.streaming_tool_result = None;
                    app.streaming_status.clear();
                }
                Ok(crate::core::agent::stream_response::StreamEvent::ToolResult { name, content }) => {
                    // Store the completed tool result for display during streaming
                    app.current_tool_call = None;
                    app.streaming_tool_result = Some((name, content));
                }
                Ok(crate::core::agent::stream_response::StreamEvent::Status(msg)) => {
                    // Show a waiting indicator during inter-turn pauses.
                    // Do NOT clear streaming_tool_result here — it was just set by
                    // ToolResult events in the same batch and needs to be rendered
                    // by the UI on the next frame. The renderer handles truncation.
                    app.streaming_status = msg;
                }
                Ok(crate::core::agent::stream_response::StreamEvent::ReasoningActive(active)) => {
                    app.is_reasoning_active = active;
                    if !active {
                        if !app.streaming_reasoning.is_empty() {
                            // Reasoning just ended — the upcoming text should be
                            // separated from any previous content by a newline.
                            if !app.streaming_text.is_empty() {
                                app.streaming_text.push_str("\n");
                            }
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
