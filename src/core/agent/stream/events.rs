use tokio::sync::mpsc;

use crate::app::App;

/// Extract the first bold (Markdown) element in the form **...** from `s`.
/// Returns the inner text if found; otherwise `None`.
/// Matches Codex's `extract_first_bold` behavior exactly.
fn extract_first_bold(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i + 1 < bytes.len() {
        if bytes[i] == b'*' && bytes[i + 1] == b'*' {
            let start = i + 2;
            let mut j = start;
            while j + 1 < bytes.len() {
                if bytes[j] == b'*' && bytes[j + 1] == b'*' {
                    // Found closing **
                    let inner = &s[start..j];
                    let trimmed = inner.trim();
                    if !trimmed.is_empty() {
                        return Some(trimmed.to_string());
                    } else {
                        return None;
                    }
                }
                j += 1;
            }
            // No closing; stop searching (wait for more deltas)
            return None;
        }
        i += 1;
    }
    None
}

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
                    // Clear tool result, todos, and status when new text arrives — model is responding
                    app.streaming_tool_result = None;
                    app.streaming_todos = None;
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
                    // If this is a todos result, also persist it in streaming_todos
                    // so it stays visible beyond the single `.take()` in the renderer.
                    if content.starts_with("## 📋 Todos") {
                        app.streaming_todos = Some(content.clone());
                    }
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
                            if app.streaming_text.is_empty() {
                                // Pre-text thinking segment: merge to last_reasoning
                                // so it displays above the streaming text.
                                if !app.last_reasoning.is_empty() {
                                    app.last_reasoning.push('\n');
                                }
                                app.last_reasoning.push_str(&app.streaming_reasoning);
                            } else {
                                // Post-text thinking (text already started):
                                // Record a text segment boundary so rendering can
                                // interleave text chunks with thinking sections
                                // (thought→text→thought→text...).
                                app.text_segment_boundaries.push(app.streaming_text.len());
                                // First archive any previous post-text segment so
                                // that each reasoning block renders as its own
                                // separate area instead of being merged together.
                                if !app.post_text_reasoning.is_empty() {
                                    app.completed_post_text_segments
                                        .push(std::mem::take(&mut app.post_text_reasoning));
                                }
                                // Then save the new segment's content.
                                app.post_text_reasoning.push_str(&app.streaming_reasoning);
                            }
                            app.streaming_reasoning.clear();
                        }
                    }
                }
                Ok(crate::core::agent::stream_response::StreamEvent::ReasoningDelta(delta)) => {
                    // NOTE: Archived post-text reasoning is handled in
                    // ReasoningActive(false) — not here — because that event
                    // is the definitive signal that a segment has ended.
                    // ReasoningDelta fires for every SSE chunk with reasoning
                    // content and cannot reliably distinguish "start of a new
                    // segment" from "continuation of the current one".

                    // Some API providers send FULL accumulated reasoning_content
                    // in each SSE chunk rather than incremental deltas.
                    if delta.starts_with(&app.streaming_reasoning) {
                        app.streaming_reasoning = delta;
                    } else {
                        app.streaming_reasoning.push_str(&delta);
                    }
                    // Extract the first bold (**...**) header for status bar display (Codex-style).
                    if let Some(header) = extract_first_bold(&app.streaming_reasoning) {
                        app.streaming_reasoning_header = Some(header);
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
