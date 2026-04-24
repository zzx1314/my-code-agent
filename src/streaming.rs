use colored::*;
use futures::StreamExt;
use rig::agent::MultiTurnStreamItem;
use rig::completion::Message;
use rig::streaming::StreamedAssistantContent;
use std::io::Write;
use termimad::MadSkin;

use crate::preamble::Agent;
use crate::ui::print_reasoning_summary;
use my_code_agent::token_usage::{print_turn_usage, TokenUsage};
use rig::streaming::StreamingChat;

/// Result of streaming a response from the agent.
pub struct StreamResult {
    pub full_response: String,
    pub interrupted: bool,
    pub should_exit: bool,
    pub last_reasoning: String,
}

/// Streams a response from the agent, handling Ctrl+C interrupts.
/// Returns the accumulated response text, whether it was interrupted, and whether the user wants to exit.
///
/// Note: if interrupted, `FinalResponse` is never received, so `chat_history` won't be
/// updated with this turn. That's acceptable — the user chose to discard the partial
/// response, and the next turn starts fresh contextually.
#[allow(unused_assignments)] // is_reasoning is set to false by end_reasoning_segment! before break paths
pub async fn stream_response(
    agent: &Agent,
    input: &str,
    chat_history: &mut Vec<Message>,
    session_usage: &mut TokenUsage,
    interrupt_rx: &mut tokio::sync::mpsc::Receiver<()>,
) -> StreamResult {

    let streaming_request = agent.stream_chat(input, chat_history.as_slice());
    let mut stream = streaming_request.await;

    let mut full_response = String::new();
    let mut interrupted = false;
    let mut is_reasoning = false;
    let mut reasoning_buf = String::new(); // buffered reasoning text for current segment (cleared after summary)
    let mut total_reasoning = String::new(); // accumulated reasoning across entire stream (for 'think' command)

    // Markdown streaming state
    let skin = MadSkin::default();
    let mut complete_lines = String::new(); // complete lines rendered through termimad
    let mut current_line = String::new(); // current incomplete line being streamed raw

    /// Flushes all buffered text (complete lines + current line) through termimad.
    /// Called when a text segment ends (tool call, final response, stream end).
    ///
    /// Known limitation: if the raw current_line wraps across multiple physical
    /// terminal lines, `\x1b[2K` only erases the cursor's current physical line,
    /// leaving orphaned wrapped lines visible briefly until the Markdown render
    /// overwrites them. This is rare in typical LLM output where lines are short.
    macro_rules! flush_all_markdown {
        () => {{
            if !current_line.is_empty() {
                print!("\r\x1b[2K");
                let _ = std::io::stdout().flush();
                complete_lines.push_str(&current_line);
                current_line.clear();
            }
            if !complete_lines.is_empty() {
                skin.print_text(&complete_lines);
                complete_lines.clear();
            }
        }};
    }

    /// Ends the current reasoning segment: prints summary, accumulates into
    /// total_reasoning, and clears the buffer.
    macro_rules! end_reasoning_segment {
        () => {{
            is_reasoning = false;
            print_reasoning_summary(&reasoning_buf);
            if !reasoning_buf.is_empty() {
                total_reasoning.push_str(&reasoning_buf);
                total_reasoning.push('\n');
            }
            reasoning_buf.clear();
        }};
    }

    loop {
        let item = tokio::select! {
            _ = interrupt_rx.recv() => {
                flush_all_markdown!();
                println!(
                    "\n  {} {}",
                    "⚠".bright_yellow(),
                    "Interrupted — press Ctrl+C again to quit".dimmed()
                );
                let second_interrupt = tokio::select! {
                    _ = tokio::time::sleep(std::time::Duration::from_millis(500)) => false,
                    _ = interrupt_rx.recv() => true,
                };
                // Flush current reasoning segment so 'think' command can access it
                if !reasoning_buf.is_empty() {
                    total_reasoning.push_str(&reasoning_buf);
                    total_reasoning.push('\n');
                }
                if second_interrupt {
                    return StreamResult {
                        full_response,
                        interrupted: true,
                        should_exit: true,
                        last_reasoning: total_reasoning,
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
                if is_reasoning {
                    end_reasoning_segment!();
                }
                full_response.push_str(&text_content.text);

                // Line-buffered Markdown streaming:
                // - Complete lines (ending with \n) are rendered through termimad
                // - The current incomplete line is printed raw for instant feedback
                let new_text = &text_content.text;
                if new_text.contains('\n') {
                    // Erase the raw current line before rendering (only if one exists)
                    if !current_line.is_empty() {
                        print!("\r\x1b[2K");
                        let _ = std::io::stdout().flush();
                    }

                    // Split new text at the last newline
                    let last_nl = new_text.rfind('\n').unwrap();
                    let before_last_nl = &new_text[..=last_nl]; // includes the \n
                    let after_last_nl = &new_text[last_nl + 1..];

                    // Add current_line + before_last_nl to complete_lines and render
                    complete_lines.push_str(&current_line);
                    complete_lines.push_str(before_last_nl);
                    current_line.clear();
                    skin.print_text(&complete_lines);
                    complete_lines.clear();

                    // Remaining partial line becomes the new current_line
                    current_line.push_str(after_last_nl);
                    if !current_line.is_empty() {
                        print!("{}", current_line);
                        let _ = std::io::stdout().flush();
                    }
                } else {
                    // No newline — accumulate into current_line, print raw
                    current_line.push_str(new_text);
                    print!("{}", new_text);
                    let _ = std::io::stdout().flush();
                }
            }
            Ok(MultiTurnStreamItem::StreamAssistantItem(
                StreamedAssistantContent::ToolCall { tool_call, .. },
            )) => {
                if is_reasoning {
                    end_reasoning_segment!();
                }
                // Flush all remaining text through Markdown before showing tool indicator
                flush_all_markdown!();
                // Preserve blank line between reasoning/text and tool indicator
                println!(
                    "\n  {} {}",
                    "⟳".bright_yellow(),
                    format!("[{}]", tool_call.function.name)
                        .bright_yellow()
                        .bold()
                );
            }
            Ok(MultiTurnStreamItem::StreamAssistantItem(
                StreamedAssistantContent::Reasoning(reasoning),
            )) => {
                if !is_reasoning {
                    is_reasoning = true;
                    print!("\n  {} ", "💭".bright_magenta());
                    print!("{}", "Thinking...".bright_magenta().dimmed());
                    let _ = std::io::stdout().flush();
                }
                let text = reasoning.display_text();
                reasoning_buf.push_str(&text);
            }
            Ok(MultiTurnStreamItem::StreamAssistantItem(
                StreamedAssistantContent::ReasoningDelta { reasoning: delta, .. },
            )) => {
                if !is_reasoning {
                    is_reasoning = true;
                    print!("\n  {} ", "💭".bright_magenta());
                    print!("{}", "Thinking...".bright_magenta().dimmed());
                    let _ = std::io::stdout().flush();
                }
                reasoning_buf.push_str(&delta);
                // No spinner needed — the "Thinking..." label above is sufficient.
                // print_reasoning_summary will cleanly replace this line when done.
            }
            Ok(MultiTurnStreamItem::FinalResponse(final_res)) => {
                if is_reasoning {
                    end_reasoning_segment!();
                }
                // Flush all remaining text through Markdown rendering
                flush_all_markdown!();
                if let Some(history) = final_res.history() {
                    *chat_history = history.to_vec();
                }
                let turn_usage = final_res.usage();
                print_turn_usage(&turn_usage);
                session_usage.add(turn_usage);
            }
            Ok(_) => {}
            Err(e) => {
                // Flush reasoning so 'think' command can access it
                if is_reasoning {
                    end_reasoning_segment!();
                }
                // Flush all remaining text through Markdown rendering
                flush_all_markdown!();
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

    // Safety-net flush for unexpected stream termination (normal path already flushes
    // in FinalResponse/Err handlers). No-op if buffers are already empty.
    flush_all_markdown!();

    StreamResult {
        full_response,
        interrupted,
        should_exit: false,
        last_reasoning: total_reasoning,
    }
}
