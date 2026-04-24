use anyhow::Result;
use colored::*;
use rustyline::error::ReadlineError;
use rustyline::history::DefaultHistory;
use rustyline::{Config as RustylineConfig, Editor};

use my_code_agent::core::config::Config;
use my_code_agent::core::context::{expand_file_refs, print_attachments};
use my_code_agent::core::token_usage::TokenUsage;
use my_code_agent::core::preamble::{build_agent, check_api_key};
use my_code_agent::core::session::{SessionData, print_resume_summary, print_saved_confirmation};
use my_code_agent::core::streaming::stream_response;
use my_code_agent::ui::{
    parse_command, print_banner, print_interrupted_notice, run_command, Command,
};

#[tokio::main]
async fn main() -> Result<()> {
    dotenv::dotenv().ok();
    check_api_key();

    let config = Config::load();

    print_banner();
    let agent = build_agent(&config);

    // Try to resume a saved session
    let session_path = SessionData::session_path(&config).to_string();
    let mut chat_history: Vec<rig::completion::Message>;
    let mut session_usage: TokenUsage;
    let mut last_reasoning: String;

    match SessionData::load_from_file(&session_path) {
        Some(Ok(data)) => {
            print_resume_summary(&data);
            chat_history = data.chat_history;
            session_usage = data.token_usage;
            last_reasoning = data.last_reasoning;
        }
        Some(Err(e)) => {
            eprintln!(
                "  {} {}",
                "⚠".bright_yellow(),
                format!("could not resume session: {}", e).dimmed()
            );
            chat_history = Vec::new();
            session_usage = TokenUsage::with_config(&config);
            last_reasoning = String::new();
        }
        None => {
            chat_history = Vec::new();
            session_usage = TokenUsage::with_config(&config);
            last_reasoning = String::new();
        }
    }

    // Line editor with proper backspace, arrow keys, and history support
    let rl_config = RustylineConfig::builder()
        .color_mode(rustyline::ColorMode::Forced)
        .build();
    let mut rl: Editor<(), DefaultHistory> = Editor::with_config(rl_config)?;

    // Interrupt channel for Ctrl+C during streaming
    let (interrupt_tx, mut interrupt_rx) = tokio::sync::mpsc::channel::<()>(1);
    tokio::spawn(async move {
        loop {
            tokio::signal::ctrl_c().await.ok();
            interrupt_tx.send(()).await.ok();
        }
    });

    loop {
        let prompt = format!("{} ", "❯".bright_green().bold());

        // Read input using rustyline on a blocking thread
        let (returned_rl, readline) = tokio::task::spawn_blocking(move || {
            let readline = rl.readline(&prompt);
            (rl, readline)
        })
        .await?;

        rl = returned_rl;

        // Drain stale interrupt signals (Ctrl+C during input also triggers the signal handler)
        while interrupt_rx.try_recv().is_ok() {}

        let input = match readline {
            Ok(line) => {
                let trimmed = line.trim().to_string();
                if !trimmed.is_empty() {
                    let _ = rl.add_history_entry(line.as_str());
                }
                trimmed
            }
            Err(ReadlineError::Interrupted) => {
                // Ctrl+C during input — save session and exit
                save_session(&session_path, &chat_history, &session_usage, &last_reasoning);
                println!("{}", "Goodbye! 👋".dimmed());
                break;
            }
            Err(ReadlineError::Eof) => {
                // Ctrl+D during input — save session and exit
                save_session(&session_path, &chat_history, &session_usage, &last_reasoning);
                println!("{}", "Goodbye! 👋".dimmed());
                break;
            }
            Err(err) => {
                anyhow::bail!("readline error: {}", err);
            }
        };

        if input.is_empty() {
            continue;
        }

        if let Some(cmd) = parse_command(&input) {
            match cmd {
                Command::Clear => {
                    chat_history.clear();
                    last_reasoning.clear();
                    // Delete the saved session file so it doesn't resume old history
                    if let Err(e) = SessionData::delete_file(&session_path) {
                        eprintln!("  {} {}", "⚠".bright_yellow(), e);
                    }
                    println!("{}", "Conversation history cleared 🗑️".dimmed());
                }
                Command::Save => {
                    save_session(&session_path, &chat_history, &session_usage, &last_reasoning);
                }
                Command::Quit => {
                    save_session(&session_path, &chat_history, &session_usage, &last_reasoning);
                    println!("{}", "Goodbye! 👋".dimmed());
                    break;
                }
                _ => {
                    run_command(cmd, &mut session_usage, &last_reasoning);
                }
            }
            continue;
        }

        let expand_result = expand_file_refs(&input, &config);
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
            // Double-Ctrl+C during streaming — the current turn was discarded,
            // but prior conversation history should still be saved.
            save_session(&session_path, &chat_history, &session_usage, &last_reasoning);
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

/// Saves the current session to disk if there is any conversation history.
fn save_session(
    path: &str,
    chat_history: &[rig::completion::Message],
    session_usage: &TokenUsage,
    last_reasoning: &str,
) {
    if chat_history.is_empty() {
        return;
    }
    let data = SessionData::new(
        chat_history.to_vec(),
        session_usage.clone(),
        last_reasoning.to_string(),
    );
    match data.save_to_file(path) {
        Ok(()) => print_saved_confirmation(path, &data),
        Err(e) => eprintln!("  {} {}", "⚠".bright_yellow(), e),
    }
}
