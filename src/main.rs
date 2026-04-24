use anyhow::Result;
use colored::*;
use rustyline::error::ReadlineError;
use rustyline::history::DefaultHistory;
use rustyline::{Config as RustylineConfig, Editor};

use my_code_agent::core::config::Config;
use my_code_agent::core::context::{expand_file_refs, print_attachments};
use my_code_agent::core::token_usage::TokenUsage;
use my_code_agent::core::preamble::{build_agent, check_api_key};
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

    let mut chat_history: Vec<rig::completion::Message> = Vec::new();
    let mut session_usage = TokenUsage::with_config(&config);
    let mut last_reasoning = String::new();

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
                // Ctrl+C during input — cancel current line, show new prompt
                continue;
            }
            Err(ReadlineError::Eof) => {
                // Ctrl+D during input — exit
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
            let is_clear = matches!(cmd, Command::Clear);
            if run_command(cmd, &mut chat_history, &mut session_usage, &last_reasoning) {
                break;
            }
            if is_clear {
                last_reasoning.clear();
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
