use colored::*;
use futures::StreamExt;
use rig::agent::MultiTurnStreamItem;
use rig::completion::Message;
use rig::streaming::StreamedAssistantContent;

use super::context_manager::ContextManager;
use super::preamble::Agent;
use super::token_usage::{TokenUsage, print_context_warning, print_turn_usage};
use crate::ui::render::{MarkdownRenderer, ReasoningTracker};
use rig::streaming::StreamingChat;

// ─────────────────────────────────────────────────────────────────────────────
// StreamResult & stream_response
// ─────────────────────────────────────────────────────────────────────────────

/// Result of streaming a response from the agent.
pub struct StreamResult {
    pub full_response: String,
    pub interrupted: bool,
    pub should_exit: bool,
    pub last_reasoning: String,
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

    loop {
        let item = tokio::select! {
            _ = interrupt_rx.recv() => {
                renderer.flush();
                println!(
                    "\n  {} {}",
                    "⚠".bright_yellow(),
                    "Interrupted — press Ctrl+C again to quit".dimmed()
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
                full_response.push_str(&text_content.text);
                renderer.push_text(&text_content.text);
            }
            Ok(MultiTurnStreamItem::StreamAssistantItem(StreamedAssistantContent::ToolCall {
                tool_call,
                ..
            })) => {
                if reasoning.is_reasoning() {
                    reasoning.end_segment();
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
    }
}
