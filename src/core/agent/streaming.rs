use tokio::sync::mpsc;

use crate::core::client::LlmClient;
use crate::core::config::AgentConfig;
use crate::core::context_manager::ContextManager;
use crate::core::token_usage::{TokenUsage, format_context_warning, format_turn_usage};
use crate::core::tool::ToolRegistry;
use crate::core::types::{Message, ToolCall};
use crate::ui::render::ReasoningTracker;

const TOOL_RESULT_OVERHEAD: u64 = 3000;

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
    ToolCall(String),
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
    chat_stream: &mut crate::core::client::ChatStream,
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
    let mut after_tool_call = false;
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
            }

            if let Some(ref text) = delta.content {
                if !text.is_empty() {
                    if reasoning_active || reasoning.is_reasoning() {
                        reasoning_active = false;
                        reasoning.end_segment();
                        send_event(StreamEvent::ReasoningActive(false));
                    }
                    let out = if after_tool_call {
                        after_tool_call = false;
                        format!("\n{}", text)
                    } else {
                        text.clone()
                    };
                    send_event(StreamEvent::Text(out.clone()));
                    response_text.push_str(&out);
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
                        send_event(StreamEvent::ToolCall(acc.name.clone().unwrap_or_else(|| "tool".to_string())));
                    }
                }
                after_tool_call = true;
                *running_approx += TOOL_RESULT_OVERHEAD;
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

    loop {
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
                    crate::core::context_cache::global_cache().record_turn(&usage);

                    let input_tokens = session_usage.last_turn_input_tokens();
                    let api_at_limit = context_manager.should_compact(input_tokens);
                    let estimated_at_limit = context_manager.is_prune_triggered();

                    if api_at_limit || estimated_at_limit {
                        if api_at_limit {
                            status_messages.push("📝 Context window full - pruning old messages...".to_string());
                        } else {
                            status_messages.push("📝 Tool-heavy turn - pruning to maintain context headroom...".to_string());
                        }
                        let pruned = context_manager.prune_messages(&messages);
                        let pruned_count = messages.len().saturating_sub(pruned.len());
                        messages = pruned;
                        // Reset the one-shot flag — it was set during SSE
                        // streaming to signal pruning for THIS turn. Keeping
                        // it true would re-enter this block every subsequent
                        // turn, spamming the user with unnecessary messages.
                        context_manager.set_prune_triggered(false);
                        context_manager.increment_compact_count();
                        status_messages.push(format!("✓ Pruned {} old messages ({} remaining)", pruned_count, messages.len()));
                        let pruned_estimate = context_manager.estimate_messages_tokens(&messages, true);
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
                    send_event(StreamEvent::ToolCall(tc.function.name.clone()));
                    let args: serde_json::Value = serde_json::from_str(&tc.function.arguments)
                        .unwrap_or(serde_json::Value::Null);
                    let result = tools.execute(&tc.function.name, args).await;
                    let content = match result {
                        Ok(output) => output,
                        Err(e) => format!("Error: {}", e),
                    };
                    let tr = Message::tool(&tc.id, content);
                    running_approx += ContextManager::estimate_message_tokens(&tr);
                    messages.push(tr);
                }
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
            messages.drain(pos..end.min(messages.len()));
        }
    }
}
