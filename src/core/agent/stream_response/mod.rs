//! Manages the multi-turn streaming conversation with the LLM.
//!
//! This module orchestrates the full response lifecycle: streaming SSE
//! chunks from the LLM, executing tool calls, detecting loops, compacting
//! context when needed, and assembling the final [`StreamResult`].

mod context;
mod loop_detection;
mod sse;
mod summarize;
mod types;

use tokio::sync::mpsc;

use crate::core::agent::client::LlmClient;
use crate::core::config::AgentConfig;
use crate::core::context::context_manager::ContextManager;
use crate::core::context::token_usage::{TokenUsage, format_context_warning, format_turn_usage};
use crate::core::types::Message;
use crate::tools::ToolRegistry;
use crate::ui::render::ReasoningTracker;

use loop_detection::ToolCallHistory;
use sse::{ProcessResult, process_sse_stream};
use summarize::generate_context_summary;
pub use types::{StreamEvent, StreamResult};

pub async fn stream_response(
    client: &LlmClient,
    system_prompt: &str,
    input: &str,
    chat_history: &mut Vec<Message>,
    tools: &ToolRegistry,
    session_usage: &mut TokenUsage,
    interrupt_rx: &mut tokio::sync::broadcast::Receiver<()>,
    context_manager: &mut ContextManager,
    agent_config: &AgentConfig,
    event_tx: Option<mpsc::UnboundedSender<StreamEvent>>,
    reasoning_field: &str,
) -> StreamResult {
    let display_mode = agent_config.thinking_display.as_str();
    let mut reasoning = ReasoningTracker::new_with_config(&agent_config.thinking_display);
    let mut status_messages: Vec<String> = Vec::new();
    let mut turn_usage_line: Option<String> = None;

    let send_event = |ev: StreamEvent| {
        if let Some(ref tx) = event_tx {
            let _ = tx.send(ev);
        }
    };

    if input.trim().is_empty() {
        status_messages.push("✗ Empty user input — ignoring".to_string());
        return StreamResult {
            full_response: String::new(),
            interrupted: false,
            should_exit: false,
            last_reasoning: reasoning.into_total_reasoning(),
            status_messages,
            turn_usage_line: None,
            session_usage: session_usage.clone(),
            updated_history: chat_history.clone(),
        };
    }

    let mut messages = chat_history.clone();

    if matches!(messages.last(), Some(Message { role, .. }) if role == "user") {
        if let Some(last) = messages.last_mut() {
            last.content = input.to_string();
        }
    } else {
        messages.push(Message::user(input));
    }

    let mut running_approx = context_manager.estimate_messages_tokens(chat_history, true)
        + ContextManager::estimate_message_tokens(&Message::user(input));

    let max_turns = agent_config.max_turns;
    let mut turn_count: usize = 0;
    let mut loop_detector = ToolCallHistory::new();

    loop {
        turn_count += 1;
        context_manager.trim(&mut messages);

        let mut api_messages = vec![Message::system(system_prompt)];
        api_messages.extend_from_slice(&messages);

        let has_tool_calls = api_messages.iter().any(|m| m.tool_calls.is_some());
        let has_tool_results = api_messages.iter().any(|m| m.tool_call_id.is_some());

        if has_tool_calls && has_tool_results {
            if let Some(last_tool) = api_messages
                .iter_mut()
                .rev()
                .find(|m| m.tool_call_id.is_some())
            {
                last_tool.content.push_str(
                    "\n\nAll tool calls above have been executed. Continue with your \
                     response. Do NOT repeat any tool call or re-analyze previous \
                     tool outputs.",
                );
            }
        }

        let tool_defs = tools.definitions();
        let mut chat_stream = match client
            .stream_chat(&api_messages, &tool_defs, reasoning_field)
            .await
        {
            Ok(s) => s,
            Err(e) => {
                status_messages.push(format!("✗ Failed to start stream: {}", e));
                return StreamResult {
                    full_response: String::new(),
                    interrupted: false,
                    should_exit: false,
                    last_reasoning: reasoning.into_total_reasoning(),
                    status_messages,
                    turn_usage_line: None,
                    session_usage: session_usage.clone(),
                    updated_history: chat_history.clone(),
                };
            }
        };

        let result = process_sse_stream(
            &mut chat_stream,
            &mut reasoning,
            &send_event,
            display_mode,
            &mut running_approx,
            context_manager,
            &mut status_messages,
            interrupt_rx,
        )
        .await;

        match result {
            ProcessResult::Interrupted => {
                reasoning.flush_unfinished();
                status_messages
                    .push("⚠ Interrupted response — press Ctrl+C again to quit".to_string());
                let second = tokio::select! {
                    _ = tokio::time::sleep(std::time::Duration::from_millis(500)) => false,
                    _ = interrupt_rx.recv() => true,
                };
                if second {
                    return StreamResult {
                        full_response: String::new(),
                        interrupted: true,
                        should_exit: true,
                        last_reasoning: reasoning.into_total_reasoning(),
                        status_messages,
                        turn_usage_line: None,
                        session_usage: session_usage.clone(),
                        updated_history: chat_history.clone(),
                    };
                }
                return StreamResult {
                    full_response: String::new(),
                    interrupted: true,
                    should_exit: false,
                    last_reasoning: reasoning.into_total_reasoning(),
                    status_messages,
                    turn_usage_line: None,
                    session_usage: session_usage.clone(),
                    updated_history: chat_history.clone(),
                };
            }
            ProcessResult::Error(err) => {
                status_messages.push(format!("✗ Stream error: {}", err));
                return StreamResult {
                    full_response: String::new(),
                    interrupted: false,
                    should_exit: false,
                    last_reasoning: reasoning.into_total_reasoning(),
                    status_messages,
                    turn_usage_line: None,
                    session_usage: session_usage.clone(),
                    updated_history: chat_history.clone(),
                };
            }
            ProcessResult::Complete {
                response_text,
                tool_calls,
                usage,
            } => {
                if reasoning.is_reasoning() && display_mode != "hidden" {
                    reasoning.end_segment();
                    send_event(StreamEvent::ReasoningActive(false));
                }

                if let Some(usage) = usage {
                    tracing::info!(
                        turn_input_tokens = usage.input_tokens,
                        turn_output_tokens = usage.output_tokens,
                        turn_total_tokens = usage.total_tokens,
                        cached_input_tokens = usage.cached_input_tokens,
                        "Turn token usage",
                    );
                    turn_usage_line = Some(format_turn_usage(&usage));
                    session_usage.add(usage);
                    crate::core::context::context_cache::global_cache().record_turn(&usage);

                    let input_tokens = session_usage.last_turn_input_tokens();
                    let api_at_limit = context_manager.should_compact(input_tokens);
                    let estimated_at_limit = context_manager.is_prune_triggered();

                    if api_at_limit || estimated_at_limit {
                        if api_at_limit {
                            status_messages.push(
                                "📝 Context window full - compacting old messages...".to_string(),
                            );
                        } else {
                            status_messages.push(
                                "📝 Tool-heavy turn - compacting to maintain context headroom..."
                                    .to_string(),
                            );
                        }

                        let mut compacted = false;
                        if context_manager.compact_count() == 0 {
                            if let Some(compact_point) =
                                context_manager.find_compact_point_percent(&messages, 30)
                            {
                                match generate_context_summary(
                                    client,
                                    &messages[..compact_point],
                                    reasoning_field,
                                )
                                .await
                                {
                                    Ok(summary) => {
                                        messages =
                                            context_manager.compact_messages(&messages, &summary);
                                        compacted = true;
                                        status_messages.push(format!(
                                            "✓ Summarized {} old messages into a compact summary ({} remaining)",
                                            compact_point,
                                            messages.len(),
                                        ));
                                    }
                                    Err(e) => {
                                        tracing::warn!(error = %e, "Summarization failed, falling back to pruning");
                                    }
                                }
                            }
                        }

                        if !compacted {
                            let pruned = context_manager.prune_messages(&messages);
                            let pruned_count = messages.len().saturating_sub(pruned.len());
                            messages = pruned;
                            status_messages.push(format!(
                                "✓ Pruned {} old messages ({} remaining)",
                                pruned_count,
                                messages.len()
                            ));
                        }

                        context_manager.set_prune_triggered(false);
                        context_manager.increment_compact_count();
                        let pruned_estimate =
                            context_manager.estimate_messages_tokens(&messages, true);
                        running_approx = pruned_estimate;
                        session_usage.update_pruned_estimate(pruned_estimate);
                    }

                    status_messages.extend(format_context_warning(session_usage));
                }

                let reasoning_text = reasoning.total_reasoning().to_string();
                let has_reasoning = !reasoning_text.is_empty();

                if tool_calls.is_empty() {
                    let assistant_msg = if has_reasoning {
                        Message::assistant_with_reasoning(&response_text, &reasoning_text)
                    } else {
                        Message::assistant(&response_text)
                    };
                    messages.push(assistant_msg);
                    *chat_history = messages;
                    return StreamResult {
                        full_response: response_text,
                        interrupted: false,
                        should_exit: false,
                        last_reasoning: reasoning.into_total_reasoning(),
                        status_messages,
                        turn_usage_line,
                        session_usage: session_usage.clone(),
                        updated_history: chat_history.clone(),
                    };
                }

                if turn_count >= max_turns {
                    status_messages.push(format!(
                        "⚠ Max turns ({}) reached — stopping further tool execution. Response may be incomplete.",
                        max_turns,
                    ));
                    let assistant_msg = if has_reasoning {
                        Message::assistant_with_reasoning(&response_text, &reasoning_text)
                    } else {
                        Message::assistant(&response_text)
                    };
                    messages.push(assistant_msg);
                    *chat_history = messages;
                    return StreamResult {
                        full_response: response_text,
                        interrupted: false,
                        should_exit: false,
                        last_reasoning: reasoning.into_total_reasoning(),
                        status_messages,
                        turn_usage_line,
                        session_usage: session_usage.clone(),
                        updated_history: chat_history.clone(),
                    };
                }

                reasoning.reset_total();

                let assistant_msg = if has_reasoning {
                    Message::assistant_with_tool_calls_and_reasoning(
                        &response_text,
                        tool_calls.clone(),
                        &reasoning_text,
                    )
                } else {
                    Message::assistant_with_tool_calls(&response_text, tool_calls.clone())
                };
                messages.push(assistant_msg);

                let mut end_turn_requested = false;

                for tc in &tool_calls {
                    send_event(StreamEvent::ToolCall {
                        name: tc.function.name.clone(),
                        arguments: tc.function.arguments.clone(),
                    });

                    if loop_detector.is_repeat_of_last(&tc.function.name, &tc.function.arguments) {
                        let repeat_count = loop_detector
                            .consecutive_repeat_count(&tc.function.name, &tc.function.arguments)
                            + 1;
                        let content = format!(
                            "[LOOP DETECTED] You've called `{}` with the same arguments {} times in a row. \
                             The previous result is still in the conversation. \
                             Review it and proceed with the next step — do NOT repeat this call.",
                            tc.function.name, repeat_count,
                        );
                        messages.push(Message::tool(&tc.id, content));
                        loop_detector.record(&tc.function.name, &tc.function.arguments);
                        continue;
                    }

                    if let Some(msg) = loop_detector.build_loop_message(&tc.function.name) {
                        messages.push(Message::tool(&tc.id, msg));
                        loop_detector.record(&tc.function.name, &tc.function.arguments);
                        continue;
                    }

                    if let Some(msg) = loop_detector.detect_alternating_pattern() {
                        messages.push(Message::tool(&tc.id, msg));
                        loop_detector.record(&tc.function.name, &tc.function.arguments);
                        continue;
                    }
                    loop_detector.record(&tc.function.name, &tc.function.arguments);

                    let args: serde_json::Value = match serde_json::from_str(&tc.function.arguments) {
                        Ok(v) => v,
                        Err(e) => {
                            let args_len = tc.function.arguments.len();
                            // Use char-level slicing to avoid panicking on multi-byte UTF-8 (e.g. Chinese)
                            let preview = if args_len > 200 {
                                let first_100: String =
                                    tc.function.arguments.chars().take(100).collect();
                                let last_100: String = tc.function.arguments
                                    .chars()
                                    .rev()
                                    .take(100)
                                    .collect::<Vec<_>>()
                                    .into_iter()
                                    .rev()
                                    .collect();
                                format!("{}...(truncated)...{}", first_100, last_100)
                            } else {
                                tc.function.arguments.chars().take(200).collect()
                            };
                            tracing::error!(
                                tool = %tc.function.name,
                                args_len = args_len,
                                error = %e,
                                "Tool call arguments are malformed JSON"
                            );
                            let content = format!(
                                "[TOOL_ERROR] `{}` failed: invalid JSON arguments (length: {}, parse error: {}).\nArguments preview: `{}`\nStop retrying this tool call — the arguments are malformed and cannot be parsed.\n\nTry to split the large content into smaller chunks instead.",
                                tc.function.name, args_len, e, preview,
                            );
                            messages.push(Message::tool(&tc.id, content));
                            // Remove the malformed tool call from the assistant message so it
                            // doesn't get re-serialized and sent to the LLM provider on the next
                            // request. If we keep it, client.rs falls back to Value::String for
                            // the unparseable arguments, which many providers reject with
                            // "Can only get item pairs from a mapping" (they expect a JSON object).
                            if let Some(last_assistant) = messages
                                .iter_mut()
                                .rev()
                                .find(|m| m.role == "assistant")
                            {
                                if let Some(ref mut calls) = last_assistant.tool_calls {
                                    calls.retain(|c| c.id != tc.id);
                                    if calls.is_empty() {
                                        last_assistant.tool_calls = None;
                                    }
                                }
                            }
                            continue;
                        }
                    };
                    let result = tools.execute(&tc.function.name, args).await;
                    let content = match result {
                        Ok(output) => output,
                        Err(e) => {
                            tracing::error!(
                                tool = %tc.function.name,
                                arguments = %tc.function.arguments,
                                error = %e,
                                "Tool call failed"
                            );
                            format!(
                                "[TOOL_ERROR] `{}` failed: {}.\nStop retrying this tool call — the operation did not succeed. Re-evaluate your approach instead.",
                                tc.function.name, e,
                            )
                        }
                    };

                    if serde_json::from_str::<serde_json::Value>(&content)
                        .ok()
                        .and_then(|v| v.get("__end_turn__").and_then(|v| v.as_bool()))
                        .unwrap_or(false)
                    {
                        end_turn_requested = true;
                    }

                    send_event(StreamEvent::ToolResult {
                        name: tc.function.name.clone(),
                        content: content.clone(),
                    });
                    messages.push(Message::tool(&tc.id, content));
                }

                if end_turn_requested {
                    status_messages.push("✓ Turn ended by assistant".to_string());
                    *chat_history = messages;
                    return StreamResult {
                        full_response: response_text,
                        interrupted: false,
                        should_exit: false,
                        last_reasoning: reasoning_text.clone(),
                        status_messages,
                        turn_usage_line,
                        session_usage: session_usage.clone(),
                        updated_history: chat_history.clone(),
                    };
                }

                send_event(StreamEvent::Status(
                    "⏳ Waiting for model response...".to_string(),
                ));
            }
        }
    }
}
