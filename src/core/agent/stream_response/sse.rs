use crate::core::agent::client::ChatStream;
use crate::core::context::context_manager::ContextManager;
use crate::core::types::{FinishReason, ToolCall};
use crate::ui::render::{ReasoningTracker, StatefulTagStripper, strip_model_metadata};

use super::types::StreamEvent;

// ─────────────────────────────────────────────────────────────────────────────
// Internal types used only within the SSE processing pipeline
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Default)]
pub(super) struct AccumToolCall {
    pub id: Option<String>,
    pub name: Option<String>,
    pub arguments: String,
}

pub(super) enum ProcessResult {
    Complete {
        response_text: String,
        tool_calls: Vec<ToolCall>,
        usage: Option<crate::core::types::Usage>,
    },
    Error(String),
    Interrupted,
}

pub(super) fn build_tool_calls(acc: &[AccumToolCall]) -> Vec<ToolCall> {
    acc.iter()
        .filter_map(|a| {
            Some(ToolCall {
                id: a.id.clone()?,
                type_: "function".to_string(),
                function: crate::core::types::ToolCallFunction {
                    name: a.name.clone()?,
                    arguments: a.arguments.clone(),
                },
            })
        })
        .collect()
}

/// Processes the SSE (Server-Sent Events) chat stream from the LLM.
///
/// Reads chunks from `chat_stream` in a loop, dispatching each chunk's delta
/// content through `send_event` and accumulating the final response text and
/// tool calls.  The loop is interruptible via `interrupt_rx`.
///
/// ### Delivery
/// - **Reasoning content** (`reasoning_content` / `reasoning`): stripped of
///   control tags by [`StatefulTagStripper`], forwarded as
///   [`StreamEvent::ReasoningDelta`] and tracked in `reasoning`.
/// - **Text content** (`content`): stripped of control tags, forwarded as
///   [`StreamEvent::Text`] and appended to `response_text`.
/// - **Tool calls** (`tool_calls`): accumulated in `acc_tool_calls` and
///   forwarded as [`StreamEvent::ToolCall`].  When accumulated size is near
///   the context window, compaction is flagged via `context_manager`.
///
/// ### Returns
/// Returns [`ProcessResult::Complete`] on natural stop / tool-call /
/// length-limit; [`ProcessResult::Interrupted`] when the user cancels; or
/// [`ProcessResult::Error`] for transport or content-filter errors.
///
pub(super) async fn process_sse_stream(
    chat_stream: &mut ChatStream,
    reasoning: &mut ReasoningTracker,
    send_event: &impl Fn(StreamEvent),
    display_mode: &str,
    running_approx: &mut u64,
    context_manager: &mut ContextManager,
    status_messages: &mut Vec<String>,
    interrupt_rx: &mut tokio::sync::broadcast::Receiver<()>,
) -> ProcessResult {
    let mut response_text = String::new();
    let mut acc_tool_calls: Vec<AccumToolCall> = Vec::new();
    let mut usage: Option<crate::core::types::Usage> = None;
    let mut reasoning_active = false;
    let mut tag_stripper = StatefulTagStripper::new();

    loop {
        let chunk = tokio::select! {
            _ = interrupt_rx.recv() => {
                return ProcessResult::Interrupted;
            }
            chunk = chat_stream.next() => {
                match chunk {
                    Some(Ok(c)) => c,
                    Some(Err(e)) => return ProcessResult::Error(e.to_string()),
                    None => return ProcessResult::Complete {
                        response_text,
                        tool_calls: build_tool_calls(&acc_tool_calls),
                        usage,
                    },
                }
            }
        };

        for choice in &chunk.choices {
            let delta = &choice.delta;

            if let Some(ref rt) = delta.reasoning_content {
                if !rt.is_empty() && display_mode != "hidden" {
                    reasoning_active = true;
                    let cleaned = strip_model_metadata(&tag_stripper.process(rt));
                    reasoning.append(&cleaned);
                    send_event(StreamEvent::ReasoningActive(true));
                    send_event(StreamEvent::ReasoningDelta(cleaned));
                }
            } else if let Some(ref rt) = delta.reasoning {
                if !rt.is_empty() && display_mode != "hidden" {
                    reasoning_active = true;
                    let cleaned = strip_model_metadata(&tag_stripper.process(rt));
                    reasoning.append(&cleaned);
                    send_event(StreamEvent::ReasoningActive(true));
                    send_event(StreamEvent::ReasoningDelta(cleaned));
                }
            }

            if let Some(ref text) = delta.content {
                if !text.is_empty() {
                    if reasoning_active || reasoning.is_reasoning() {
                        reasoning_active = false;
                        reasoning.end_segment();
                        send_event(StreamEvent::ReasoningActive(false));
                    }
                    let cleaned = strip_model_metadata(&tag_stripper.process(text));
                    send_event(StreamEvent::Text(cleaned.clone()));
                    response_text.push_str(&cleaned);
                    *running_approx += ContextManager::estimate_text_tokens(&cleaned);
                }
            }

            if let Some(ref tcds) = delta.tool_calls {
                if reasoning.is_reasoning() {
                    reasoning.end_segment();
                }
                if reasoning_active {
                    reasoning_active = false;
                    send_event(StreamEvent::ReasoningActive(false));
                }
                for tcd in tcds {
                    let idx = tcd.index as usize;
                    while acc_tool_calls.len() <= idx {
                        acc_tool_calls.push(AccumToolCall::default());
                    }
                    let acc = &mut acc_tool_calls[idx];
                    if let Some(ref id) = tcd.id {
                        acc.id = Some(id.clone());
                    }
                    if let Some(ref name) = tcd.function.as_ref().and_then(|f| f.name.as_ref()) {
                        acc.name = Some(name.to_string());
                    }
                    if let Some(ref args) = tcd.function.as_ref().and_then(|f| f.arguments.as_ref())
                    {
                        // Some providers (via rig-core) emit both ToolCallDelta fragments AND
                        // a final complete ToolCall for the same call. If the new args look like
                        // a complete JSON object and we already have accumulated content,
                        // treat it as a replacement (not a delta append) to avoid duplication:
                        //   {"path":".","max_depth":2}{"max_depth":2,"path":"."}
                        if !acc.arguments.is_empty() && args.starts_with('{') && args.ends_with('}')
                        {
                            acc.arguments = args.to_string();
                        } else {
                            acc.arguments.push_str(args);
                        }
                    }
                    if acc.name.is_some() {
                        send_event(StreamEvent::ToolCall {
                            name: acc.name.clone().unwrap_or_else(|| "tool".to_string()),
                            arguments: acc.arguments.clone(),
                        });
                    }
                }
                if context_manager.should_compact(*running_approx)
                    && !context_manager.is_prune_triggered()
                {
                    context_manager.set_prune_triggered(true);
                    status_messages.push(
                        "📝 Context window nearly full — will compact after this turn".to_string(),
                    );
                }
            }
        }

        if let Some(ref u) = chunk.usage {
            usage = Some(*u);
        }

        if let Some(ref reason) = chunk.choices.iter().find_map(|c| c.finish_reason.as_ref()) {
            tracing::info!(mapped = ?reason, "Stream chunk finished");

            match reason {
                FinishReason::Stop => {
                    return ProcessResult::Complete {
                        response_text,
                        tool_calls: build_tool_calls(&acc_tool_calls),
                        usage,
                    };
                }
                FinishReason::ToolCalls => {
                    return ProcessResult::Complete {
                        response_text,
                        tool_calls: build_tool_calls(&acc_tool_calls),
                        usage,
                    };
                }
                FinishReason::Length => {
                    let msg = "⚠ Response was truncated due to token length limit. \
                               Consider breaking the task into smaller steps."
                        .to_string();
                    status_messages.push(msg.clone());
                    send_event(StreamEvent::Status(msg));
                    return ProcessResult::Complete {
                        response_text,
                        tool_calls: build_tool_calls(&acc_tool_calls),
                        usage,
                    };
                }
                FinishReason::ContentFilter => {
                    return ProcessResult::Error(
                        "Response was filtered by content moderation policy.".to_string(),
                    );
                }
                FinishReason::Unknown(original) => {
                    tracing::warn!(
                        reason = %original,
                        "Unknown finish_reason from LLM — treating as stop"
                    );
                    return ProcessResult::Complete {
                        response_text,
                        tool_calls: build_tool_calls(&acc_tool_calls),
                        usage,
                    };
                }
            }
        }
    }
}
