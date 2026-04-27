use colored::*;
use futures::StreamExt;
use rig::agent::MultiTurnStreamItem;
use rig::completion::{CompletionModel, GetTokenUsage, Message};
use rig::streaming::{StreamedAssistantContent, StreamingChat};

use super::config::AgentConfig;
use super::context_manager::ContextManager;
use super::plan_tracker::PlanTracker;
use super::token_usage::{TokenUsage, print_context_warning, print_turn_usage};
use crate::core::preamble::Agent;
use crate::ui::render::{MarkdownRenderer, ReasoningTracker};

// ─────────────────────────────────────────────────────────────────────────────
// StreamResult
// ─────────────────────────────────────────────────────────────────────────────

pub struct StreamResult {
    pub full_response: String,
    pub interrupted: bool,
    pub should_exit: bool,
    pub last_reasoning: String,
    pub plan_tracker: PlanTracker,
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
    interrupt_rx: &mut tokio::sync::mpsc::Receiver<()>,
    context_manager: &mut ContextManager,
    agent_config: &AgentConfig,
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
    interrupt_rx: &mut tokio::sync::mpsc::Receiver<()>,
    context_manager: &mut ContextManager,
    agent_config: &AgentConfig,
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
    let mut renderer = MarkdownRenderer::new();
    let mut reasoning = ReasoningTracker::new_with_config(&agent_config.thinking_display);
    let mut plan_tracker = PlanTracker::new();
    let mut plan_detected = false;
    let mut plan_text = String::new();

    let display_mode = agent_config.thinking_display.as_str();

    // Animation timer: update bouncing ellipsis every 100ms
    let mut anim_interval = tokio::time::interval(std::time::Duration::from_millis(100));
    anim_interval.tick().await; // Skip immediate first tick

    loop {
        let item = tokio::select! {
            // Animation update tick
            _ = anim_interval.tick() => {
                if display_mode != "hidden" && reasoning.is_reasoning() {
                    reasoning.update_animation();
                }
                continue;
            }

            _ = interrupt_rx.recv() => {
                renderer.flush();
                println!(
                    "\n  {} {}",
                    "⚠".bright_yellow(),
                    "Interrupt response — press Ctrl+C again to quit".dimmed()
                );
                let second_interrupt = tokio::select! {
                    _ = tokio::time::sleep(std::time::Duration::from_millis(500)) => false,
                    _ = interrupt_rx.recv() => true,
                };
                reasoning.flush_unfinished();
                if second_interrupt {
                    return StreamResult {
                        full_response,
                        interrupted: true,
                        should_exit: true,
                        last_reasoning: reasoning.into_total_reasoning(),
                        plan_tracker,
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
                }

                if !plan_detected && detect_task_plan(&text_content.text) {
                    plan_detected = true;
                    plan_text.clear();
                    renderer.flush();
                    // Don't print the heading here - let it stream through renderer
                    plan_text.push_str(&text_content.text);
                    continue;  // Accumulate, don't print yet
                }

                if plan_detected {
                    plan_text.push_str(&text_content.text);
                    let lines: Vec<&str> = plan_text.lines().collect();
                    let mut in_plan = true;
                    let mut plan_ended = false;
                    for line in lines {
                        let trimmed = line.trim();
                        let is_numbered = trimmed.len() > 2
                            && trimmed.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false)
                            && trimmed.chars().nth(1) == Some('.');

                        if is_numbered && in_plan {
                            renderer.push_text(line);
                            renderer.push_text("\n");
                        } else {
                            if in_plan && !trimmed.is_empty() && !is_numbered {
                                plan_tracker.parse_plan(&plan_text);
                                in_plan = false;
                                plan_ended = true;
                            }
                            renderer.push_text(line);
                            if !line.is_empty() {
                                renderer.push_text("\n");
                            }
                        }
                    }
                    if plan_ended {
                        plan_text.clear();
                    }
                } else {
                    renderer.push_text(&text_content.text);
                }

                full_response.push_str(&text_content.text);
            }

            Ok(MultiTurnStreamItem::StreamAssistantItem(StreamedAssistantContent::ToolCall {
                tool_call,
                ..
            })) => {
                if reasoning.is_reasoning() {
                    reasoning.end_segment();
                }
                if plan_tracker.has_active_plan() && plan_tracker.is_confirmed() {
                    plan_tracker.complete_current_step();
                    plan_tracker.print_progress();
                    println!();
                }
                renderer.flush();
                println!(
                    "\n  {} {}",
                    "⟳".bright_yellow(),
                    format!("[{}]", tool_call.function.name).bright_yellow().bold()
                );
            }

            Ok(MultiTurnStreamItem::StreamAssistantItem(StreamedAssistantContent::Reasoning(
                r,
            ))) => {
                if display_mode != "hidden" {
                    reasoning.append(&r.display_text());
                }
            }

            Ok(MultiTurnStreamItem::StreamAssistantItem(
                StreamedAssistantContent::ReasoningDelta {
                    reasoning: delta, ..
                },
            )) => {
                if display_mode != "hidden" {
                    reasoning.append(&delta);
                }
            }

            Ok(MultiTurnStreamItem::FinalResponse(final_res)) => {
                if reasoning.is_reasoning() && display_mode != "hidden" {
                    reasoning.end_segment();
                }
                renderer.flush();

                if plan_tracker.has_active_plan() {
                    println!("\n  {} {}", "📋".bright_green(), "Task Plan".bold());
                    plan_tracker.print_completion();
                }

                if let Some(history) = final_res.history() {
                    *chat_history = history.to_vec();
                }
                let turn_usage = final_res.usage();
                print_turn_usage(&turn_usage);
                session_usage.add(turn_usage);
                print_context_warning(session_usage);

                let input_tokens = session_usage.input_tokens();
                if context_manager.should_compact(input_tokens) {
                    println!(
                        "\n  {} {}",
                        "📝".bright_cyan(),
                        "Context window full - pruning old messages...".dimmed()
                    );
                    let pruned_messages = context_manager.prune_messages(chat_history);
                    let pruned_count = chat_history.len() - pruned_messages.len();
                    *chat_history = pruned_messages;
                    context_manager.set_prune_triggered(true);
                    context_manager.increment_compact_count();
                    println!(
                        "  {} Pruned {} old messages ({} remaining)",
                        "✓".bright_green(),
                        pruned_count,
                        chat_history.len()
                    );
                }
            }

            Ok(_) => {}

            Err(e) => {
                if reasoning.is_reasoning() {
                    reasoning.end_segment();
                }
                renderer.flush();
                let err_msg = e.to_string();
                if err_msg.contains("MaxTurnError") || err_msg.contains("max turn limit") {
                    if full_response.is_empty() {
                        println!(
                            "\n{} {}",
                            "⚠".bright_yellow(),
                            "Reached tool call limit without producing a response.".dimmed()
                        );
                    } else {
                        println!(
                            "\n{} {}",
                            "⚠".bright_yellow(),
                            "Reached tool call limit. Here is what I have so far:".dimmed()
                        );
                    }
                } else {
                    eprintln!("\n{} Error: {}", "✗".bright_red(), e);
                }
                break;
            }
        }
    }

    renderer.flush();

    StreamResult {
        full_response,
        interrupted,
        should_exit: false,
        last_reasoning: reasoning.into_total_reasoning(),
        plan_tracker,
    }
}