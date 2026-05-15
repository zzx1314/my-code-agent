use std::collections::VecDeque;
use tokio::sync::mpsc;

use crate::core::agent::client::LlmClient;
use crate::core::config::AgentConfig;
use crate::core::context::context_manager::ContextManager;
use crate::core::context::token_usage::{TokenUsage, format_context_warning, format_turn_usage};
use crate::tools::ToolRegistry;
use crate::core::types::{Message, ToolCall};
use crate::ui::render::ReasoningTracker;

// ─────────────────────────────────────────────────────────────────────────────
// Tool call history — detects repeated identical calls to break loops
// ─────────────────────────────────────────────────────────────────────────────

/// Tracks recent tool calls to detect when the model repeats itself.
struct ToolCallHistory {
    /// Recent (name, normalized_args) pairs. Most recent at the back.
    calls: VecDeque<(String, String)>,
    /// Max entries to keep.
    max_len: usize,
}

impl ToolCallHistory {
    fn new() -> Self {
        Self {
            calls: VecDeque::with_capacity(4),
            max_len: 4,
        }
    }

    /// Record a tool call.
    fn record(&mut self, name: &str, args: &str) {
        let normalized = Self::normalize(args);
        self.calls.push_back((name.to_string(), normalized));
        while self.calls.len() > self.max_len {
            self.calls.pop_front();
        }
    }

    /// Check if this call is identical to the previous one.
    fn is_repeat_of_last(&self, name: &str, args: &str) -> bool {
        let normalized = Self::normalize(args);
        self.calls.back().map_or(false, |(n, a)| n == name && a == &normalized)
    }

    /// Normalize arguments: sort keys so semantically identical JSON matches.
    fn normalize(args: &str) -> String {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(args) {
            // Sort object keys for deterministic comparison
            if let serde_json::Value::Object(map) = v {
                let mut keys: Vec<&String> = map.keys().collect();
                keys.sort();
                let sorted: Vec<String> = keys
                    .iter()
                    .map(|k| {
                        let val = &map[*k];
                        format!("\"{}\":{}", k, val)
                    })
                    .collect();
                return format!("{{{}}}", sorted.join(","));
            }
        }
        args.to_string()
    }

    /// Check if this call has been made multiple times in a row (for the status message).
    fn consecutive_repeat_count(&self, name: &str, args: &str) -> usize {
        let normalized = Self::normalize(args);
        let mut count = 0;
        for (n, a) in self.calls.iter().rev() {
            if n == name && a == &normalized {
                count += 1;
            } else {
                break;
            }
        }
        count
    }

    /// Count how many consecutive calls have been made to the same tool,
    /// regardless of argument differences. This catches trial-and-error loops
    /// where the model calls e.g. `file_update` repeatedly with slightly
    /// different `old` strings.
    fn consecutive_same_tool_count(&self, name: &str) -> usize {
        let mut count = 0;
        for (n, _) in self.calls.iter().rev() {
            if n == name {
                count += 1;
            } else {
                break;
            }
        }
        count
    }

    /// Build a diagnostic message explaining the loop pattern to the model.
    fn build_loop_message(&self, name: &str) -> Option<String> {
        let same_tool_count = self.consecutive_same_tool_count(name);
        if same_tool_count >= 2 {
            Some(format!(
                "[LOOP DETECTED] You have called `{}` {} times in a row (with different arguments each time). \
                 The previous calls did not produce the expected result. \
                 Stop and reassess: re-read the file to see its current state, \
                 then try a different approach.",
                name,
                same_tool_count + 1,
            ))
        } else {
            None
        }
    }
}

#[derive(Clone)]
pub struct StreamResult {
    pub full_response: String,
    pub interrupted: bool,
    pub should_exit: bool,
    pub last_reasoning: String,
    pub status_messages: Vec<String>,
    pub turn_usage_line: Option<String>,
    pub session_usage: TokenUsage,
    pub updated_history: Vec<Message>,
}

#[derive(Debug, Clone)]
pub enum StreamEvent {
    Text(String),
    ToolCall { name: String, arguments: String },
    ToolResult { name: String, content: String },
    /// Status update for inter-turn waiting periods (e.g. "Waiting for model...")
    /// Clears the previous streaming_tool_result so the UI doesn't render heavy content.
    Status(String),
    ReasoningActive(bool),
    ReasoningDelta(String),
}

#[derive(Default)]
struct AccumToolCall {
    id: Option<String>,
    name: Option<String>,
    arguments: String,
}

async fn process_sse_stream(
    chat_stream: &mut crate::core::agent::client::ChatStream,
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
                    reasoning.append(rt);
                    send_event(StreamEvent::ReasoningActive(true));
                    send_event(StreamEvent::ReasoningDelta(rt.clone()));
                }
            } else if let Some(ref rt) = delta.reasoning {
                if !rt.is_empty() && display_mode != "hidden" {
                    reasoning_active = true;
                    reasoning.append(rt);
                    send_event(StreamEvent::ReasoningActive(true));
                    send_event(StreamEvent::ReasoningDelta(rt.clone()));
                }
            }

            if let Some(ref text) = delta.content {
                if !text.is_empty() {
                    if reasoning_active || reasoning.is_reasoning() {
                        reasoning_active = false;
                        reasoning.end_segment();
                        send_event(StreamEvent::ReasoningActive(false));
                    }
                    send_event(StreamEvent::Text(text.clone()));
                    response_text.push_str(text);
                    *running_approx += ContextManager::estimate_text_tokens(text);
                }
            }

            if let Some(ref tcds) = delta.tool_calls {
                if reasoning.is_reasoning() {
                    reasoning.end_segment();
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
                    if let Some(ref args) = tcd.function.as_ref().and_then(|f| f.arguments.as_ref()) {
                        acc.arguments.push_str(args);
                    }
                    if acc.name.is_some() {
                        send_event(StreamEvent::ToolCall {
                            name: acc.name.clone().unwrap_or_else(|| "tool".to_string()),
                            arguments: acc.arguments.clone(),
                        });
                    }
                }
                if context_manager.should_compact(*running_approx) && !context_manager.is_prune_triggered() {
                    context_manager.set_prune_triggered(true);
                    status_messages.push("📝 Context window nearly full — will compact after this turn".to_string());
                }
            }
        }

        if let Some(ref u) = chunk.usage {
            usage = Some(*u);
        }

        if chunk.choices.iter().any(|c| c.finish_reason.is_some()) {
            return ProcessResult::Complete {
                response_text,
                tool_calls: build_tool_calls(&acc_tool_calls),
                usage,
            };
        }
    }
}

fn build_tool_calls(acc: &[AccumToolCall]) -> Vec<ToolCall> {
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

enum ProcessResult {
    Complete {
        response_text: String,
        tool_calls: Vec<ToolCall>,
        usage: Option<crate::core::types::Usage>,
    },
    Error(String),
    Interrupted,
}

/// Generate a concise summary of old conversation messages using a non-streaming
/// LLM call. Called during context compaction on the first compaction event to
/// preserve semantic content while saving tokens.
///
/// Returns the summary text on success. The caller should fall back to
/// [`ContextManager::prune_messages`] if this function fails.
async fn generate_context_summary(
    client: &LlmClient,
    old_messages: &[Message],
) -> anyhow::Result<String> {
    let summary_prompt = Message::user(
        "Please provide a concise summary of the above conversation. \
         Focus on: user goals, decisions made, files changed, key findings, \
         and any important context that would help continue the work. \
         Keep the summary under 300 words and write in the same language as the conversation."
    );    let mut api_messages = vec![
            Message::system(
                "You are a helpful assistant that summarizes technical conversations concisely. \
                 Preserve all important context, decisions, and file paths."
            ),
        ];
    api_messages.extend_from_slice(old_messages);
    api_messages.push(summary_prompt);

    let response = client.chat(&api_messages, &[]).await?;

    let content = response["choices"][0]["message"]["content"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("No content in summary response"))?
        .to_string();

    Ok(content)
}

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

        let tool_defs = tools.definitions();
        let mut chat_stream = match client.stream_chat(&api_messages, &tool_defs).await {
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
                status_messages.push("⚠ Interrupted response — press Ctrl+C again to quit".to_string());
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
                            status_messages.push("📝 Context window full - compacting old messages...".to_string());
                        } else {
                            status_messages.push("📝 Tool-heavy turn - compacting to maintain context headroom...".to_string());
                        }

                        // Try LLM summarization on FIRST compaction to preserve
                        // semantic content; fall back to pruning on subsequent
                        // compactions or if the LLM call fails.
                        let mut compacted = false;
                        if context_manager.compact_count() == 0 {
                            if let Some(compact_point) = context_manager.find_compact_point(&messages) {
                                match generate_context_summary(client, &messages[..compact_point]).await {
                                    Ok(summary) => {
                                        messages = context_manager.compact_messages(&messages, &summary);
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
                            status_messages.push(format!("✓ Pruned {} old messages ({} remaining)", pruned_count, messages.len()));
                        }

                        // Reset the one-shot flag — it was set during SSE
                        // streaming to signal pruning for THIS turn. Keeping
                        // it true would re-enter this block every subsequent
                        // turn, spamming the user with unnecessary messages.
                        context_manager.set_prune_triggered(false);
                        context_manager.increment_compact_count();
                        let pruned_estimate = context_manager.estimate_messages_tokens(&messages, true);
                        running_approx = pruned_estimate;
                        session_usage.update_pruned_estimate(pruned_estimate);
                    }

                    status_messages.extend(format_context_warning(session_usage));
                }

                // Capture the full reasoning content for this assistant turn.
                // DeepSeek requires `reasoning_content` to be echoed back in
                // subsequent API requests when using reasoning models.
                // For non-reasoning models, `reasoning_text` will be empty and
                // we fall back to standard constructors without `reasoning_content`.
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

                // Enforce max_turns: if we've reached the limit, stop executing
                // more tools and return the accumulated response text.
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

                // Reset the per-turn reasoning tracker so the next loop
                // iteration starts fresh. Without this, total_reasoning
                // accumulates across every tool-call turn, causing each
                // subsequent assistant message to carry ALL prior turns'
                // reasoning_content — both incorrect and wasteful.
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

                for tc in &tool_calls {
                    send_event(StreamEvent::ToolCall {
                        name: tc.function.name.clone(),
                        arguments: tc.function.arguments.clone(),
                    });

                    // ── Loop detection ─────────────────────────────────
                    // Two patterns are detected:
                    // 1. Exact repeat: same tool + same args (e.g. file_read
                    //    with same offset/limit).
                    // 2. Same-tool spiral: same tool called 3+ times in a
                    //    row with different args (e.g. file_update with
                    //    slightly different `old` strings each time).
                    if loop_detector.is_repeat_of_last(&tc.function.name, &tc.function.arguments) {
                        let repeat_count =
                            loop_detector.consecutive_repeat_count(&tc.function.name, &tc.function.arguments)
                            + 1;
                        let content = format!(
                            "[LOOP DETECTED] You've called `{}` with the same arguments {} times in a row. \
                             The previous result is still in the conversation. \
                             Review it and proceed with the next step — do NOT repeat this call.",
                            tc.function.name,
                            repeat_count,
                        );
                        let tr = Message::tool(&tc.id, content);
                        messages.push(tr);
                        loop_detector.record(&tc.function.name, &tc.function.arguments);
                        continue;
                    }

                    // Check for same-tool spiral: same tool 3+ times
                    if let Some(msg) = loop_detector.build_loop_message(&tc.function.name) {
                        let tr = Message::tool(&tc.id, msg);
                        messages.push(tr);
                        loop_detector.record(&tc.function.name, &tc.function.arguments);
                        continue;
                    }
                    loop_detector.record(&tc.function.name, &tc.function.arguments);

                    let args: serde_json::Value = serde_json::from_str(&tc.function.arguments)
                        .unwrap_or(serde_json::Value::Null);
                    let result = tools.execute(&tc.function.name, args).await;
                    let content = match result {
                        Ok(output) => output,
                        Err(e) => format!("Error: {}", e),
                    };
                    // Emit tool result event for real-time display during streaming
                    send_event(StreamEvent::ToolResult {
                        name: tc.function.name.clone(),
                        content: content.clone(),
                    });
                    let tr = Message::tool(&tc.id, content);
                    messages.push(tr);
                }

                // After all tools have executed and their results sent, signal
                // the UI to clear the heavy tool result content and show a
                // waiting indicator while the model processes the results.
                send_event(StreamEvent::Status("⏳ Waiting for model response...".to_string()));

            }
        }
    }
}

impl ContextManager {
    pub fn trim(&self, messages: &mut Vec<Message>) {
        while self.estimate_messages_tokens(messages, false) > self.estimate_max_tokens() {
            Self::drop_oldest_round(messages);
        }
    }

    fn drop_oldest_round(messages: &mut Vec<Message>) {
        if let Some(pos) = messages.iter().position(|m| m.role == "user") {
            let end = messages[pos + 1..]
                .iter()
                .position(|m| m.role == "user")
                .map(|i| pos + 1 + i)
                .unwrap_or(pos + 2);
            // Extend past any tool messages that belong to the removed
            // assistant, to avoid leaving orphaned tool messages which
            // DeepSeek rejects.
            let mut actual_end = end.min(messages.len());
            while actual_end < messages.len() && messages[actual_end].role == "tool" {
                actual_end += 1;
            }
            messages.drain(pos..actual_end);
        }
    }
}
