use colored::*;
use rig::completion::Message;

use crate::preamble::KNOWLEDGE_FILE;
use my_code_agent::token_usage::TokenUsage;

/// Prints the startup banner.
pub fn print_banner() {
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
        "file_read · file_write · file_update · file_delete · shell_exec · code_search"
            .bright_green()
    );
    println!(
        "  {} {}",
        "Knowledge:".bright_white().bold(),
        format!("auto-loads {} into context", KNOWLEDGE_FILE).bright_green()
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

/// Prints the help menu.
pub fn print_help() {
    println!();
    println!("  {}  Read file contents", "file_read".bright_yellow());
    println!("  {}  Write to a file", "file_write".bright_yellow());
    println!(
        "  {}  Edit existing files (find & replace)",
        "file_update".bright_yellow()
    );
    println!(
        "  {}  Delete files, directories, or code snippets",
        "file_delete".bright_yellow()
    );
    println!("  {}  Run shell commands", "shell_exec".bright_yellow());
    println!("  {}  Search code patterns", "code_search".bright_yellow());
    println!();
    println!("  {}  Show token usage statistics", "usage".dimmed());
    println!("  {}  Clear conversation history", "clear".dimmed());
    println!(
        "  {}  Expand last reasoning content",
        "think".bright_magenta()
    );
    println!("  {}  Exit the agent", "quit".dimmed());
    println!();
    println!(
        "  {}  Auto-loaded into every conversation",
        KNOWLEDGE_FILE.bright_cyan()
    );
    println!(
        "  {}  Attach file contents to your message",
        "@<filepath>".bright_cyan()
    );
    println!();
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

/// Prints a notice if the response was interrupted.
pub fn print_interrupted_notice(full_response: &str, interrupted: bool) {
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

/// Built-in commands that don't require an agent response.
pub enum Command {
    Help,
    Usage,
    Clear,
    Quit,
    Think,
}

/// Checks whether the input is a built-in command.
/// Returns `Some(Command)` if recognized, `None` otherwise.
pub fn parse_command(input: &str) -> Option<Command> {
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
pub fn run_command(
    cmd: Command,
    chat_history: &mut Vec<Message>,
    session_usage: &mut TokenUsage,
    last_reasoning: &str,
) -> bool {
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
