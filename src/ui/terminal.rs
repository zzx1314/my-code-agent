use crate::core::preamble::KNOWLEDGE_FILE;
use crate::core::token_usage::TokenUsage;
use colored::*;

pub fn print_banner() {
    println!();
    println!("{}", " _                               _   ".bright_cyan());
    println!(
        "{}",
        "  _ __ ___  _   _    ___ ___   __| | ___    __ _  __ _  ___ _ __ | |_ ".bright_cyan()
    );
    println!(
        "{}",
        " | '_ ` _ \\ | | | |  / __/ _ \\ / _` |/ _ \\  / _` |/ _` |/ _ \\ '_ \\| __|"
            .bright_cyan()
    );
    println!(
        "{}",
        " | | | | | | |_| | | (_| (_) | (_| |  __/ | (_| | (_| |  __/ | | | |_ ".bright_cyan()
    );
    println!(
        "{}",
        " |_| |_| |_|\\__, |  \\___\\___/ \\__,_|\\___|  \\__,_|\\__, |\\___|_| |_|\\__|"
            .bright_cyan()
    );
    println!(
        "{}",
        "            |___/                                |___/".bright_cyan()
    );
    println!();
    println!(
        "  {}",
        "🤖 My Code Agent v0.1.0 (reasoner)".bright_white().bold()
    );
    println!();
    println!(
        "  {} {}",
        "Tools:".bright_white().bold(),
        "file_read · file_write · file_update · file_delete · shell_exec · code_search · list_dir · glob"
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
    println!("  {}  List directory contents", "list_dir".bright_yellow());
    println!("  {}  Find files by glob pattern", "glob".bright_yellow());
    println!();
    println!("  {}  Show token usage statistics", "usage".dimmed());
    println!(
        "  {}  Save conversation session as <name>",
        "save".bright_green()
    );
    println!("  {}  List saved sessions", "sessions".dimmed());
    println!("  {}  Load a saved session by name", "load".dimmed());
    println!("  {}  Clear conversation history", "clear".dimmed());
    println!(
        "  {}  Expand last reasoning content",
        "think".bright_magenta()
    );
    println!("  {}  Exit the agent (auto-saves session)", "quit".dimmed());
    println!();
    println!(
        "  {}  Auto-loaded into every conversation",
        KNOWLEDGE_FILE.bright_cyan()
    );
    println!(
        "  {}  Attach file contents to your message",
        "@".bright_cyan()
    );
    println!();
}

pub fn print_reasoning_full(reasoning: &str) {
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

pub fn print_sessions_list(sessions: &[crate::core::session::SessionInfo]) {
    println!();
    println!(
        "  {} {}",
        "📂".bright_cyan(),
        "Saved Sessions".bright_white().bold()
    );
    println!("  {}", "─".repeat(50).dimmed());

    if sessions.is_empty() {
        println!("  {}", "No saved sessions.".dimmed());
        println!("  {}", "Use /save <name> to save current session.".dimmed());
    } else {
        for (i, session) in sessions.iter().enumerate() {
            let when = crate::core::session::format_timestamp(session.saved_at);
            println!(
                "  [{}] {}  {}  •  {} turns",
                (i + 1).to_string().bright_yellow(),
                session.name.bright_white(),
                when.dimmed(),
                session.turns
            );
        }
    }
    println!("  {}", "─".repeat(50).dimmed());
    println!("  {} {}", "n".bright_yellow(), "New session".dimmed());
    println!();
}

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

pub enum Command {
    Help,
    Usage,
    Clear,
    Quit,
    Think,
    Save,
    Sessions,
    Load,
}

pub fn parse_command(input: &str) -> Option<Command> {
    let input = input.strip_prefix('/').unwrap_or(input);
    match input {
        "help" => Some(Command::Help),
        "usage" => Some(Command::Usage),
        "clear" => Some(Command::Clear),
        "quit" | "exit" | "q" => Some(Command::Quit),
        "think" => Some(Command::Think),
        "save" => Some(Command::Save),
        "sessions" => Some(Command::Sessions),
        "load" => Some(Command::Load),
        _ => None,
    }
}

pub fn run_command(cmd: Command, session_usage: &mut TokenUsage, last_reasoning: &str) -> bool {
    match cmd {
        Command::Help => print_help(),
        Command::Usage => session_usage.print_session_report(),
        Command::Think => {
            print_reasoning_full(last_reasoning);
        }
        Command::Clear | Command::Quit | Command::Save | Command::Sessions | Command::Load => {}
    }
    false
}
