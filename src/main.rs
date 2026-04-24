use anyhow::Result;
use colored::*;
use std::io::Write;

use my_code_agent::core::context::{expand_file_refs, print_attachments};
use my_code_agent::core::token_usage::TokenUsage;
use my_code_agent::core::preamble::{build_agent, check_api_key};
use my_code_agent::core::streaming::stream_response;
use my_code_agent::ui::{
    parse_command, print_banner, print_interrupted_notice, run_command, Command,
};

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
    check_api_key();

    print_banner();
    let agent = build_agent();

    let mut chat_history: Vec<rig::completion::Message> = Vec::new();
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
