use colored::*;
use futures::StreamExt;
use rig::agent::MultiTurnStreamItem;
use rig::completion::Message;
use rig::streaming::StreamedAssistantContent;

use super::context_manager::ContextManager;
use super::plan_tracker::PlanTracker;
use super::token_usage::{TokenUsage, print_context_warning, print_turn_usage};
use crate::ui::render::{MarkdownRenderer, ReasoningTracker};

use rig::streaming::StreamingChat;

type Agent = rig::agent::Agent<rig::providers::deepseek::CompletionModel>;

// ─────────────────────────────────────────────────────────────────────────────
// StreamResult & stream_response
// ─────────────────────────────────────────────────────────────────────────────

/// Result of streaming a response from the agent.
pub struct StreamResult {
    pub full_response: String,
    pub interrupted: bool,
    pub should_exit: bool,
    pub last_reasoning: String,
    pub plan_tracker: PlanTracker,
}

/// Detects if the text contains a task plan header (not in code blocks)
pub fn detect_task_plan(text: &str) -> bool {
    // Skip if we're inside a code block (between ``` and ```)
    if text.contains("```") {
        // Simple heuristic: if code block marker appears before the header,
        // we're likely inside a code block
        let first_code = text.find("```").unwrap_or(usize::MAX);
        let first_header = text.find("##").unwrap_or(usize::MAX);
        if first_code < first_header {
            return false;
        }
    }

    // Check for common task plan patterns (markdown headers)
    text.contains("## 📋 Task Plan")
        || text.contains("## Task Plan")
        || text.contains("## Plan")
        || text.contains("### Plan")
}

/// Streams a response from the agent, handling Ctrl+C interrupts.
///
/// Returns the accumulated response text, whether it was interrupted, and whether
/// the user wants to exit.
///
/// Note: if interrupted, `FinalResponse` is never received, so `chat_history`
/// won't be updated with this turn. That's acceptable — the user chose to discard
/// the partial response, and the next turn starts fresh contextually.
pub async fn stream_response(
    agent: &Agent,
    input: &str,
    chat_history: &mut Vec<Message>,
    session_usage: &mut TokenUsage,
    interrupt_rx: &mut tokio::sync::mpsc::Receiver<()>,
    context_manager: &mut ContextManager,
) -> StreamResult {
    let streaming_request = agent.stream_chat(input, chat_history.as_slice());
    let mut stream = streaming_request.await;

    let mut full_response = String::new();
    let mut interrupted = false;
    let mut renderer = MarkdownRenderer::new();
    let mut reasoning = ReasoningTracker::new();
    let mut plan_tracker = PlanTracker::new();
    let mut plan_detected = false;
    let mut plan_text = String::new();

    loop {
        let item = tokio::select! {
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

                // Detect task plan header
                if !plan_detected && detect_task_plan(&text_content.text) {
                    plan_detected = true;
                    plan_text.clear();
                    renderer.flush();
                    println!("\n  {} {}", "📋".bright_green(), "Task Plan".bold());
                }

                // Track plan text and highlight steps
                if plan_detected {
                    plan_text.push_str(&text_content.text);
                    let mut in_plan = true;
                    for line in text_content.text.lines() {
                        let trimmed = line.trim();
                        // Check if this is a numbered step
                        let is_numbered = trimmed.len() > 2
                            && trimmed
                                .chars()
                                .next()
                                .map(|c| c.is_ascii_digit())
                                .unwrap_or(false)
                            && trimmed.chars().nth(1) == Some('.');

                        if is_numbered && in_plan {
                            println!("    {} {}", "→".bright_cyan(), trimmed.bright_white());
                        } else {
                            if in_plan && !trimmed.is_empty() && !is_numbered {
                                // End of plan section, print confirmation prompt
                                plan_tracker.parse_plan(&plan_text);
                                plan_tracker.print_with_confirmation();
                                plan_text.clear(); // We parsed it, clear to avoid double parsing
                                in_plan = false;
                            }
                            renderer.push_text(line);
                            if !line.is_empty() {
                                renderer.push_text("\n");
                            }
                        }
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

                // Update plan progress when tool is called
                if plan_tracker.has_active_plan() && plan_tracker.is_confirmed() {
                    plan_tracker.complete_current_step();
                    plan_tracker.print_progress();
                    println!(); // newline after progress
                }

                renderer.flush();
                println!(
                    "\n  {} {}",
                    "⟳".bright_yellow(),
                    format!("[{}]", tool_call.function.name)
                        .bright_yellow()
                        .bold()
                );
            }
            Ok(MultiTurnStreamItem::StreamAssistantItem(StreamedAssistantContent::Reasoning(
                r,
            ))) => {
                reasoning.append(&r.display_text());
            }
            Ok(MultiTurnStreamItem::StreamAssistantItem(
                StreamedAssistantContent::ReasoningDelta {
                    reasoning: delta, ..
                },
            )) => {
                reasoning.append(&delta);
            }
            Ok(MultiTurnStreamItem::FinalResponse(final_res)) => {
                if reasoning.is_reasoning() {
                    reasoning.end_segment();
                }
                renderer.flush();

                // Print plan completion if applicable
                if plan_tracker.has_active_plan() {
                    plan_tracker.print_completion();
                }

                if let Some(history) = final_res.history() {
                    *chat_history = history.to_vec();
                }
                let turn_usage = final_res.usage();
                print_turn_usage(&turn_usage);
                session_usage.add(turn_usage);
                print_context_warning(session_usage);

                // Check if context pruning is needed
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

    // Safety-net flush for unexpected stream termination (normal path already
    // flushes in FinalResponse/Err handlers). No-op if buffers are already empty.
    renderer.flush();

    StreamResult {
        full_response,
        interrupted,
        should_exit: false,
        last_reasoning: reasoning.into_total_reasoning(),
        plan_tracker,
    }
}
