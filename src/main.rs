use anyhow::Result;
use colored::*;
use futures::StreamExt;
use rig::agent::MultiTurnStreamItem;
use rig::client::{CompletionClient, ProviderClient};
use rig::completion::Message;
use rig::providers::deepseek;
use rig::streaming::StreamedAssistantContent;
use rig::streaming::StreamingChat;
use std::io::Write;

use my_code_agent::context::{expand_file_refs, print_attachments};
use my_code_agent::token_usage::{print_turn_usage, TokenUsage};
use termimad::MadSkin;

type Agent = rig::agent::Agent<deepseek::CompletionModel>;

const PREAMBLE: &str = r#"You are an expert coding assistant with access to tools for reading, writing, searching, and executing code.

## Your Capabilities
- **file_read**: Read file contents from the local filesystem
- **file_write**: Create new files on the local filesystem (for editing existing files, use file_update instead)
- **file_update**: Make targeted edits to existing files. Always read the file first with file_read to ensure the `old` string matches exactly, then use file_update to apply the edit
- **file_delete**: Delete files, directories, or specific text snippets from files. Use `snippet` to remove code without deleting the whole file. Use with caution.
- **shell_exec**: Execute shell commands (build, test, lint, etc.)
- **code_search**: Search for patterns in source code using grep
## Critical Rules
1. **STOP after answering**: Once you have gathered enough information to answer the user's question, provide a text response immediately. Do NOT call more tools.
2. **Minimum tools**: Use the fewest tool calls possible. Typically 1-3 calls per question is sufficient. Do not chain tool calls unnecessarily.
3. **No redundant exploration**: Do not read multiple files to "understand the codebase" when one file suffices. Do not run shell commands that duplicate information from file_read.
4. **Respond directly**: After using tools, give the user a clear answer. Never end a turn with only a tool call — always follow up with text.
5. **No retry loops**: If a tool call fails or returns unexpected results, explain the issue to the user. Do not retry the same call with minor variations.

## Guidelines
1. **Understand first**: Read relevant files before making changes.
2. **Be precise**: Make minimal, targeted edits. Don't rewrite entire files unnecessarily.
3. **Verify changes**: After writing code, run relevant tests or type checks.
4. **Explain your reasoning**: Briefly explain what you're doing and why.
5. **Handle errors gracefully**: If a command fails, read the error and tell the user.
6. **Use relative paths**: Prefer paths relative to the current working directory.

Always be concise but thorough."#;

fn print_banner() {
    println!();
    println!(
        "{}",
        "╔══════════════════════════════════════════╗".bright_cyan()
    );
    println!(
        "{}",
        "║     🤖  My Code Agent v0.1.0 (reasoner) ║".bright_cyan()
    );
    println!(
        "{}",
        "╚══════════════════════════════════════════╝".bright_cyan()
    );
    println!();
    println!(
        "  {} {}",
        "Tools:".bright_white().bold(),
        "file_read · file_write · file_update · file_delete · shell_exec · code_search".bright_green()
    );
    println!(
        "  {} {}",
        "Files:".bright_white().bold(),
        "@<path> to attach file contents".bright_green()
    );
    println!(
        "  {} {}",
        "Type:".bright_white().bold(),
        "your request to get started, 'help' for commands".dimmed()
    );
    println!(
        "  {} {}",
        "Ctrl+C:".bright_white().bold(),
        "interrupt response / double-press to quit".dimmed()
    );
    println!();
}

/// Reads a line from stdin on a blocking thread so it can be cancelled via `tokio::select!`.
async fn read_stdin_line() -> Option<String> {
    tokio::task::spawn_blocking(|| {
        let mut buf = String::new();
        match std::io::stdin().read_line(&mut buf) {
            Ok(0) => None, // EOF
            Ok(_) => Some(buf),
            Err(_) => None,
        }
    })
    .await
    .ok()
    .flatten()
}

/// Validates that the DEEPSEEK_API_KEY environment variable is set.
fn check_api_key() {
    if std::env::var("DEEPSEEK_API_KEY").is_err() {
        eprintln!(
            "{} DEEPSEEK_API_KEY not set. Add it to .env or your environment.",
            "✗".bright_red()
        );
        std::process::exit(1);
    }
}

/// Builds the DeepSeek agent with tools and preamble.
///
/// Precondition: `DEEPSEEK_API_KEY` must be set (enforced by `check_api_key()`).
fn build_agent() -> Agent {
    let client = deepseek::Client::from_env();
    let tools = my_code_agent::tools::all_tools();

    client
        .agent(deepseek::DEEPSEEK_REASONER)
        .preamble(PREAMBLE)
        .tools(tools)
        .default_max_turns(10)
        .build()
}

/// Prints the help menu.
fn print_help() {
    println!();
    println!("  {}  Read file contents", "file_read".bright_yellow());
    println!("  {}  Write to a file", "file_write".bright_yellow());
    println!("  {}  Edit existing files (find & replace)", "file_update".bright_yellow());
    println!("  {}  Delete files, directories, or code snippets", "file_delete".bright_yellow());
    println!("  {}  Run shell commands", "shell_exec".bright_yellow());
    println!("  {}  Search code patterns", "code_search".bright_yellow());
    println!();
    println!("  {}  Show token usage statistics", "usage".dimmed());
    println!("  {}  Clear conversation history", "clear".dimmed());
    println!("  {}  Expand last reasoning content", "think".bright_magenta());
    println!("  {}  Exit the agent", "quit".dimmed());
    println!();
    println!("  {}  Attach file contents to your message", "@<filepath>".bright_cyan());
    println!();
}

/// Built-in commands that don't require an agent response.
enum Command {
    Help,
    Usage,
    Clear,
    Quit,
    Think,
}

/// Checks whether the input is a built-in command.
/// Returns `Some(Command)` if recognized, `None` otherwise.
fn parse_command(input: &str) -> Option<Command> {
    match input {
        "help" => Some(Command::Help),
        "usage" => Some(Command::Usage),
        "clear" => Some(Command::Clear),
        "quit" | "exit" | "q" => Some(Command::Quit),
        "think" => Some(Command::Think),
        _ => None,
    }
}

/// Executes a built-in command. Returns true if the main loop should break.
fn run_command(cmd: Command, chat_history: &mut Vec<Message>, session_usage: &mut TokenUsage, last_reasoning: &str) -> bool {
    match cmd {
        Command::Help => print_help(),
        Command::Usage => session_usage.print_session_report(),
        Command::Clear => {
            chat_history.clear();
            println!("{}", "Conversation history cleared 🗑️".dimmed());
        }
        Command::Quit => {
            println!("{}", "Goodbye! 👋".dimmed());
            return true;
        }
        Command::Think => {
            print_reasoning_full(last_reasoning);
        }
    }
    false
}

/// Prints a collapsed summary of the reasoning content.
/// Shows the first line of reasoning (or a truncation hint) so the user knows reasoning occurred
/// without flooding the terminal. The full reasoning can be reviewed with the `think` command.
fn print_reasoning_summary(reasoning: &str) {
    if reasoning.is_empty() {
        return;
    }
    // Erase the "Thinking..." line first
    print!("\r\x1b[2K");
    let _ = std::io::stdout().flush();

    // Get first non-empty line as summary
    let first_line = reasoning
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("");

    let char_count = reasoning.len();
    let line_count = reasoning.lines().count();

    // Build display text, handling empty first line
    let display_line = if first_line.is_empty() {
        "(see full reasoning)".to_string()
    } else if first_line.chars().count() > 80 {
        // Truncate first line if too long (char-based to avoid UTF-8 panic)
        let truncated: String = first_line.chars().take(77).collect();
        format!("{}...", truncated)
    } else {
        first_line.to_string()
    };

    println!(
        "  {} {} ({} chars, {} lines) {}",
        "💭".bright_magenta(),
        display_line.bright_magenta().dimmed(),
        char_count.to_string().bright_magenta().dimmed(),
        line_count.to_string().bright_magenta().dimmed(),
        "[type 'think' to expand]".bright_black()
    );
}

/// Prints the full reasoning content (expanded view).
fn print_reasoning_full(reasoning: &str) {
    if reasoning.is_empty() {
        println!("  {}", "No reasoning content available.".dimmed());
        return;
    }
    println!();
    println!(
        "  {} {}",
        "💭".bright_magenta(),
        "Reasoning:".bright_magenta().bold()
    );
    println!(
        "  {}",
        "─────────────────────────────────────────"
            .bright_magenta()
            .dimmed()
    );
    for line in reasoning.lines() {
        println!("  {}", line.bright_magenta().dimmed());
    }
    println!(
        "  {}",
        "─────────────────────────────────────────"
            .bright_magenta()
            .dimmed()
    );
    println!();
}

/// Result of streaming a response from the agent.
struct StreamResult {
    full_response: String,
    interrupted: bool,
    should_exit: bool,
    last_reasoning: String,
}

/// Streams a response from the agent, handling Ctrl+C interrupts.
/// Returns the accumulated response text, whether it was interrupted, and whether the user wants to exit.
///
/// Note: if interrupted, `FinalResponse` is never received, so `chat_history` won't be
/// updated with this turn. That's acceptable — the user chose to discard the partial
/// response, and the next turn starts fresh contextually.
async fn stream_response(
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
                    is_reasoning = false;
                    // Print collapsed reasoning summary
                    print_reasoning_summary(&reasoning_buf);
                    if !reasoning_buf.is_empty() {
                        total_reasoning.push_str(&reasoning_buf);
                        total_reasoning.push('\n');
                    }
                    reasoning_buf.clear();
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
                    is_reasoning = false;
                    // Print collapsed reasoning summary
                    print_reasoning_summary(&reasoning_buf);
                    if !reasoning_buf.is_empty() {
                        total_reasoning.push_str(&reasoning_buf);
                        total_reasoning.push('\n');
                    }
                    reasoning_buf.clear();
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
                    is_reasoning = false;
                    // Print collapsed reasoning summary
                    print_reasoning_summary(&reasoning_buf);
                    if !reasoning_buf.is_empty() {
                        total_reasoning.push_str(&reasoning_buf);
                        total_reasoning.push('\n');
                    }
                    reasoning_buf.clear();
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
                    print_reasoning_summary(&reasoning_buf);
                    if !reasoning_buf.is_empty() {
                        total_reasoning.push_str(&reasoning_buf);
                        total_reasoning.push('\n');
                    }
                    reasoning_buf.clear();
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

/// Prints a notice if the response was interrupted.
fn print_interrupted_notice(full_response: &str, interrupted: bool) {
    if !full_response.is_empty() {
        println!();
        if interrupted {
            println!(
                "  {}",
                "(response was interrupted, context and token usage not recorded)".dimmed()
            );
            println!();
        } else {
            println!();
        }
    } else if interrupted {
        println!(
            "  {}",
            "(response was interrupted, token usage not recorded)".dimmed()
        );
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenv::dotenv().ok();
    check_api_key();

    let agent = build_agent();
    print_banner();

    let mut chat_history: Vec<Message> = Vec::new();
    let mut session_usage = TokenUsage::new();
    let mut last_reasoning = String::new();

    let (interrupt_tx, mut interrupt_rx) = tokio::sync::mpsc::channel::<()>(1);
    tokio::spawn(async move {
        loop {
            tokio::signal::ctrl_c().await.ok();
            interrupt_tx.send(()).await.ok();
        }
    });

    loop {
        print!("{} ", "❯".bright_green().bold());
        std::io::stdout().flush()?;

        let input: Option<String> = tokio::select! {
            _ = interrupt_rx.recv() => {
                println!("\n{}", "Goodbye! 👋".dimmed());
                return Ok(());
            }
            line = read_stdin_line() => line,
        };

        let input = match input {
            Some(line) => line.trim().to_string(),
            None => {
                println!("{}", "Goodbye! 👋".dimmed());
                break;
            }
        };

        if input.is_empty() {
            continue;
        }

        if let Some(cmd) = parse_command(&input) {
            let is_clear = matches!(cmd, Command::Clear);
            if run_command(cmd, &mut chat_history, &mut session_usage, &last_reasoning) {
                break;
            }
            // Clear reasoning when conversation is cleared
            if is_clear {
                last_reasoning.clear();
            }
            continue;
        }

        let expand_result = expand_file_refs(&input);
        if !expand_result.attachments.is_empty() {
            print_attachments(&expand_result.attachments);
        }
        let result = stream_response(
            &agent,
            &expand_result.expanded,
            &mut chat_history,
            &mut session_usage,
            &mut interrupt_rx,
        )
        .await;

        if result.should_exit {
            println!("{}", "Goodbye! 👋".dimmed());
            return Ok(());
        }

        // Drain stale interrupt signals
        while interrupt_rx.try_recv().is_ok() {}

        last_reasoning = result.last_reasoning;

        print_interrupted_notice(&result.full_response, result.interrupted);
    }

    Ok(())
}
