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

const PREAMBLE: &str = r#"You are an expert coding assistant with access to tools for reading, writing, searching, and executing code.

## Your Capabilities
- **file_read**: Read file contents from the local filesystem
- **file_write**: Write or create files on the local filesystem
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
        "║     🤖  DeepSeek Code Agent v0.1.0       ║".bright_cyan()
    );
    println!(
        "{}",
        "╚══════════════════════════════════════════╝".bright_cyan()
    );
    println!();
    println!(
        "  {} {}",
        "Tools:".bright_white().bold(),
        "file_read · file_write · shell_exec · code_search".bright_green()
    );
    println!(
        "  {} {}",
        "Type:".bright_white().bold(),
        "your request to get started, 'quit' to exit".dimmed()
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

#[tokio::main]
async fn main() -> Result<()> {
    dotenv::dotenv().ok();

    if std::env::var("DEEPSEEK_API_KEY").is_err() {
        eprintln!(
            "{} {}",
            "✗".bright_red(),
            "DEEPSEEK_API_KEY not set. Add it to .env or your environment."
        );
        std::process::exit(1);
    }

    let client = deepseek::Client::from_env();
    let tools = my_deepseek_agent::tools::all_tools();

    let agent = client
        .agent(deepseek::DEEPSEEK_CHAT)
        .preamble(PREAMBLE)
        .tools(tools)
        .default_max_turns(10)
        .build();

    print_banner();

    let mut chat_history: Vec<Message> = Vec::new();

    // Ctrl+C channel: a background task repeatedly awaits Ctrl+C and sends notifications.
    // This gives us a reusable, cancellable signal source for tokio::select!.
    let (interrupt_tx, mut interrupt_rx) = tokio::sync::mpsc::channel::<()>(1);
    tokio::spawn(async move {
        loop {
            tokio::signal::ctrl_c().await.ok();
            interrupt_tx.send(()).await.ok();
        }
    });

    loop {
        // Read user input — Ctrl+C at the prompt exits the program
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
                // EOF (Ctrl+D)
                println!("{}", "Goodbye! 👋".dimmed());
                break;
            }
        };

        if input.is_empty() {
            continue;
        }

        if input == "quit" || input == "exit" || input == "q" {
            println!("{}", "Goodbye! 👋".dimmed());
            break;
        }

        if input == "help" {
            println!();
            println!("  {}  Read file contents", "file_read".bright_yellow());
            println!("  {}  Write to a file", "file_write".bright_yellow());
            println!("  {}  Run shell commands", "shell_exec".bright_yellow());
            println!("  {}  Search code patterns", "code_search".bright_yellow());
            println!();
            println!("  {}  Clear conversation history", "clear".dimmed());
            println!("  {}  Exit the agent", "quit".dimmed());
            println!();
            continue;
        }

        if input == "clear" {
            chat_history.clear();
            println!("{}", "Conversation history cleared 🗑️".dimmed());
            continue;
        }

        // Stream the response with multi-turn history
        let streaming_request = agent.stream_chat(&input, &chat_history);
        let mut stream = streaming_request.await;

        let mut full_response = String::new();
        let mut interrupted = false;

        loop {
            let item = tokio::select! {
                _ = interrupt_rx.recv() => {
                    println!(
                        "\n{} {}",
                        "⚠".bright_yellow(),
                        "Interrupted — press Ctrl+C again to quit".dimmed()
                    );
                    // Brief pause: if user presses Ctrl+C again quickly, we exit
                    let second_interrupt = tokio::select! {
                        _ = tokio::time::sleep(std::time::Duration::from_millis(500)) => false,
                        _ = interrupt_rx.recv() => true,
                    };
                    if second_interrupt {
                        println!("{}", "Goodbye! 👋".dimmed());
                        return Ok(());
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
                    print!("{}", text_content.text);
                    std::io::stdout().flush()?;
                    full_response.push_str(&text_content.text);
                }
                Ok(MultiTurnStreamItem::StreamAssistantItem(
                    StreamedAssistantContent::ToolCall { tool_call, .. },
                )) => {
                    println!(
                        "\n  {} {}",
                        "⟳".bright_yellow(),
                        format!("[{}]", tool_call.function.name)
                            .bright_yellow()
                            .bold()
                    );
                }
                Ok(MultiTurnStreamItem::FinalResponse(final_res)) => {
                    // Update chat history with the completed turn
                    if let Some(history) = final_res.history() {
                        chat_history = history.to_vec();
                    }
                }
                Ok(_) => {
                    // Ignore other stream items (reasoning deltas, tool call deltas, etc.)
                }
                Err(e) => {
                    let err_msg = e.to_string();
                    if err_msg.contains("MaxTurnError") || err_msg.contains("max turn limit") {
                        // TODO: match on error type if rig-core exports it
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

        // Note: if interrupted, FinalResponse is never received, so chat_history
        // won't be updated with this turn. That's acceptable — the user chose to
        // discard the partial response, and the next turn starts fresh contextually.

        // Drain any stale interrupt signals so they don't trigger at the next prompt
        while interrupt_rx.try_recv().is_ok() {}

        if !full_response.is_empty() {
            println!();
            if interrupted {
                println!(
                    "  {}",
                    "(response was interrupted, context not saved to history)".dimmed()
                );
                println!();
            } else {
                println!();
            }
        }
    }

    Ok(())
}
