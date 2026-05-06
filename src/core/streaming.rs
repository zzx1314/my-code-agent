use futures::StreamExt;
use rig::agent::MultiTurnStreamItem;
use rig::completion::{CompletionModel, GetTokenUsage, Message};
use rig::streaming::{StreamedAssistantContent, StreamingChat};
use tokio::sync::mpsc;

use super::config::AgentConfig;
use super::context_manager::ContextManager;
use super::plan_tracker::PlanTracker;
use super::token_usage::{TokenUsage, format_context_warning, format_turn_usage};
use crate::core::preamble::Agent;
use crate::ui::render::ReasoningTracker;

// ─────────────────────────────────────────────────────────────────────────────
// StreamResult & StreamEvent
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct StreamResult {
    pub full_response: String,
    pub interrupted: bool,
    pub should_exit: bool,
    pub last_reasoning: String,
    pub plan_tracker: PlanTracker,
    pub status_messages: Vec<String>,
    pub turn_usage_line: Option<String>,
    pub session_usage: TokenUsage,
}

/// Events emitted during streaming for real-time UI display.
#[derive(Debug, Clone)]
pub enum StreamEvent {
    /// A chunk of response text to display incrementally.
    Text(String),
    /// A tool call is being executed.
    ToolCall(String),
    /// Reasoning is active (showing/hiding indicator).
    ReasoningActive(bool),
    /// Reasoning content delta.
    ReasoningDelta(String),
    /// Plan progress message (e.g., step status, completion).
    PlanProgress(String),
}

// ─────────────────────────────────────────────────────────────────────────────
// detect_task_plan
// ─────────────────────────────────────────────────────────────────────────────

pub fn detect_task_plan(text: &str) -> bool {
    if text.contains("```") {
        let first_code = text.find("```").unwrap_or(usize::MAX);
        let first_header = text.find("##").unwrap_or(usize::MAX);
        if first_code < first_header {
            return false;
        }
    }
    text.contains("## 📋 Task Plan")
        || text.contains("## Task Plan")
        || text.contains("## Plan")
        || text.contains("### Plan")
}

// ─────────────────────────────────────────────────────────────────────────────
// Public entry point — dispatches to stream_inner based on Agent variant
// ─────────────────────────────────────────────────────────────────────────────

pub async fn stream_response(
    agent: &Agent,
    input: &str,
    chat_history: &mut Vec<Message>,
    session_usage: &mut TokenUsage,
    interrupt_rx: &mut tokio::sync::broadcast::Receiver<()>,
    context_manager: &mut ContextManager,
    agent_config: &AgentConfig,
    event_tx: Option<mpsc::UnboundedSender<StreamEvent>>,
) -> StreamResult {
    match agent {
        Agent::OpenAI(inner) => {
            stream_inner(
                inner,
                input,
                chat_history,
                session_usage,
                interrupt_rx,
                context_manager,
                agent_config,
                event_tx,
            )
            .await
        }
        Agent::OpenRouter(inner) => {
            stream_inner(
                inner,
                input,
                chat_history,
                session_usage,
                interrupt_rx,
                context_manager,
                agent_config,
                event_tx,
            )
            .await
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Generic core — works for any CompletionModel whose Agent implements StreamingChat
// ─────────────────────────────────────────────────────────────────────────────
async fn stream_inner<M>(
    agent: &rig::agent::Agent<M>,
    input: &str,
    chat_history: &mut Vec<Message>,
    session_usage: &mut TokenUsage,
    interrupt_rx: &mut tokio::sync::broadcast::Receiver<()>,
    context_manager: &mut ContextManager,
    agent_config: &AgentConfig,
    event_tx: Option<mpsc::UnboundedSender<StreamEvent>>,
) -> StreamResult
where
    M: CompletionModel + Send + Sync + 'static,
    M::StreamingResponse: Clone + Unpin + GetTokenUsage + Send,
    rig::agent::Agent<M>: StreamingChat<M, M::StreamingResponse>,
{
    let streaming_request = agent.stream_chat(input, chat_history.as_slice());
    let mut stream = streaming_request.await;

    let mut full_response = String::new();
    let mut interrupted = false;
    let mut reasoning = ReasoningTracker::new_with_config(&agent_config.thinking_display);
    let mut plan_tracker = PlanTracker::new();
    let mut plan_detected = false;
    let mut plan_text = String::new();
    let mut status_messages: Vec<String> = Vec::new();
    let mut after_tool_call = false;

    let display_mode = agent_config.thinking_display.as_str();

    // Helper to send event if channel exists
    let send_event = |ev: StreamEvent| {
        if let Some(ref tx) = event_tx {
            let _ = tx.send(ev);
        }
    };

    loop {
        let item = tokio::select! {
            _ = interrupt_rx.recv() => {
                reasoning.flush_unfinished();
                status_messages.push("⚠ Interrupted response — press Ctrl+C again to quit".to_string());
                let second_interrupt = tokio::select! {
                    _ = tokio::time::sleep(std::time::Duration::from_millis(500)) => false,
                    _ = interrupt_rx.recv() => true,
                };
                if second_interrupt {
                    return StreamResult {
                        full_response,
                        interrupted: true,
                        should_exit: true,
                        last_reasoning: reasoning.into_total_reasoning(),
                        plan_tracker,
                        status_messages,
                        turn_usage_line: None,
                        session_usage: session_usage.clone(),
                    };
                }
                interrupted = true;
                break;
            }
            item = stream.next() => {
                match item {
                    Some(item) => item,
                    None => break,
                }
            }
        };

        match item {
            Ok(MultiTurnStreamItem::StreamAssistantItem(StreamedAssistantContent::Text(
                text_content,
            ))) => {
                if reasoning.is_reasoning() {
                    reasoning.end_segment();
                    send_event(StreamEvent::ReasoningActive(false));
                }

                let text_to_send = if after_tool_call {
                    after_tool_call = false;
                    let mut text = String::from("\n");
                    text.push_str(&text_content.text);
                    text
                } else {
                    text_content.text.clone()
                };

                // Send text delta for live display (plain text during streaming)
                send_event(StreamEvent::Text(text_to_send.clone()));

                if !plan_detected && detect_task_plan(&text_content.text) {
                    plan_detected = true;
                    plan_text.clear();
                    plan_text.push_str(&text_content.text);
                    continue;
                }

                if plan_detected {
                    plan_text.push_str(&text_content.text);
                }

                full_response.push_str(&text_to_send);
            }

            Ok(MultiTurnStreamItem::StreamAssistantItem(StreamedAssistantContent::ToolCall {
                tool_call,
                ..
            })) => {
                if reasoning.is_reasoning() {
                    reasoning.end_segment();
                }
                // If plan was detected but not yet parsed, parse it now and auto-confirm
                if plan_detected && !plan_tracker.has_active_plan() {
                    plan_tracker.parse_plan(&plan_text);
                    let plan_display = plan_tracker.format_with_confirmation();
                    status_messages.push(plan_display.clone());
                    send_event(StreamEvent::PlanProgress(plan_display));
                    plan_tracker.confirm();
                    send_event(StreamEvent::PlanProgress(
                        "✓ Plan confirmed, proceeding...".to_string(),
                    ));
                }
                // Update step statuses based on "y" markers in accumulated text
                if plan_tracker.has_active_plan() && plan_tracker.is_confirmed() {
                    plan_tracker.update_from_text(&plan_text);
                    plan_tracker.log_progress();
                    for msg in plan_tracker.take_messages() {
                        send_event(StreamEvent::PlanProgress(msg));
                    }
                    let progress = plan_tracker.progress_display();
                    if !progress.is_empty() {
                        status_messages.push(progress.clone());
                        send_event(StreamEvent::PlanProgress(progress));
                    }
                }
                // Tool calls are only displayed in real-time during streaming via StreamEvent::ToolCall (current_tool_call in render_chat_area)
                // No longer appended to full_response to avoid polluting conversation history with tool call markers
                send_event(StreamEvent::ToolCall(tool_call.function.name.clone()));
                after_tool_call = true;
            }

            Ok(MultiTurnStreamItem::StreamAssistantItem(StreamedAssistantContent::Reasoning(
                r,
            ))) => {
                if display_mode != "hidden" {
                    reasoning.append(&r.display_text());
                    send_event(StreamEvent::ReasoningActive(true));
                    send_event(StreamEvent::ReasoningDelta(r.display_text()));
                }
            }

            Ok(MultiTurnStreamItem::StreamAssistantItem(
                StreamedAssistantContent::ReasoningDelta {
                    reasoning: delta, ..
                },
            )) => {
                if display_mode != "hidden" {
                    reasoning.append(&delta);
                    send_event(StreamEvent::ReasoningActive(true));
                    send_event(StreamEvent::ReasoningDelta(delta));
                }
            }

            Ok(MultiTurnStreamItem::FinalResponse(final_res)) => {
                // Fallback: 如果 plan 被检测到但没有 tool call 触发 parse，在这里补上
                if plan_detected && !plan_tracker.has_active_plan() {
                    plan_tracker.parse_plan(&plan_text);
                    let plan_display = plan_tracker.format_with_confirmation();
                    status_messages.push(plan_display.clone());
                    send_event(StreamEvent::PlanProgress(plan_display));
                }

                if reasoning.is_reasoning() && display_mode != "hidden" {
                    reasoning.end_segment();
                }

                if plan_tracker.has_active_plan() {
                    // Final update: re-parse markers one last time
                    plan_tracker.update_from_text(&plan_text);
                    let progress = plan_tracker.progress_display();
                    status_messages.push(format!("📋 Task Plan {}", progress));
                    plan_tracker.log_completion();
                    for msg in plan_tracker.take_messages() {
                        send_event(StreamEvent::PlanProgress(msg.clone()));
                        status_messages.push(msg);
                    }
                }

                if let Some(history) = final_res.history() {
                    *chat_history = history.to_vec();
                }
                let turn_usage = final_res.usage();
                let turn_usage_line = Some(format_turn_usage(&turn_usage));
                session_usage.add(turn_usage);

                // Record cache metrics for this turn
                crate::core::context_cache::global_cache().record_turn(&turn_usage);

                status_messages.extend(format_context_warning(session_usage));

                let input_tokens = session_usage.input_tokens();
                if context_manager.should_compact(input_tokens) {
                    status_messages
                        .push("📝 Context window full - pruning old messages...".to_string());
                    let pruned_messages = context_manager.prune_messages(chat_history);
                    let pruned_count = chat_history.len() - pruned_messages.len();
                    *chat_history = pruned_messages;
                    context_manager.set_prune_triggered(true);
                    context_manager.increment_compact_count();
                    status_messages.push(format!(
                        "✓ Pruned {} old messages ({} remaining)",
                        pruned_count,
                        chat_history.len()
                    ));
                }

                return StreamResult {
                    full_response,
                    interrupted,
                    should_exit: false,
                    last_reasoning: reasoning.into_total_reasoning(),
                    plan_tracker,
                    status_messages,
                    turn_usage_line,
                    session_usage: session_usage.clone(),
                };
            }

            Ok(_) => {}

            Err(e) => {
                if reasoning.is_reasoning() {
                    reasoning.end_segment();
                }
                let err_msg = e.to_string();
                if err_msg.contains("MaxTurnError") || err_msg.contains("max turn limit") {
                    if full_response.is_empty() {
                        status_messages.push(
                            "⚠ Reached tool call limit without producing a response.".to_string(),
                        );
                    } else {
                        status_messages.push(
                            "⚠ Reached tool call limit. Here is what I have so far:".to_string(),
                        );
                    }
                } else {
                    status_messages.push(format!("✗ Error: {}", e));
                }
                break;
            }
        }
    }

    StreamResult {
        full_response,
        interrupted,
        should_exit: false,
        last_reasoning: reasoning.into_total_reasoning(),
        plan_tracker,
        status_messages,
        turn_usage_line: None,
        session_usage: session_usage.clone(),
    }
}
