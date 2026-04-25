use anyhow::Result;
use colored::*;
use my_code_agent::core::config::Config;
use my_code_agent::core::context::{expand_file_refs, print_attachments};
use my_code_agent::core::context_manager::ContextManager;
use my_code_agent::core::file_cache::FileCache;
use my_code_agent::core::preamble::{build_agent, check_api_key};
use my_code_agent::core::session::SessionData;
use my_code_agent::core::streaming::stream_response;
use my_code_agent::core::token_usage::TokenUsage;
use my_code_agent::ui::{
    Command, parse_command, print_banner, print_interrupted_notice, print_sessions_list,
    run_command,
};

use reedline::{
    ColumnarMenu, Completer, DefaultPrompt, Emacs, KeyCode, KeyModifiers, Reedline, ReedlineEvent,
    ReedlineMenu, Signal, Span, Suggestion,
};
use std::path::Path;

#[tokio::main]
async fn main() -> Result<()> {
    dotenv::dotenv().ok();
    check_api_key();

    let config = Config::load();

    print_banner();
    let agent = build_agent(&config);

    // New session by default
    let mut chat_history: Vec<rig::completion::Message> = Vec::new();
    let mut session_usage: TokenUsage = TokenUsage::with_config(&config);
    let mut last_reasoning: String = String::new();
    let mut current_session_name: Option<String> = None;

    // Initialize context manager and file cache
    let mut context_manager = ContextManager::new(&config);
    let mut file_cache = FileCache::new(50, 300);
    
    struct FilePathCompleter {
        default_completer: reedline::DefaultCompleter,
    }

    impl FilePathCompleter {
        fn new() -> Self {
            let commands = vec![
                "/help".into(),
                "/usage".into(),
                "/save".into(),
                "/sessions".into(),
                "/load".into(),
                "/clear".into(),
                "/think".into(),
                "/quit".into(),
                "@file_read".into(),
                "@file_write".into(),
                "@file_update".into(),
                "@file_delete".into(),
                "@shell_exec".into(),
                "@code_search".into(),
                "@list_dir".into(),
                "@glob".into(),
            ];
            let mut default_completer =
                reedline::DefaultCompleter::with_inclusions(&['/', '@']).set_min_word_len(1);
            default_completer.insert(commands);
            Self { default_completer }
        }

        fn complete_file_path(&self, path_prefix: &str) -> Vec<Suggestion> {
            let path_part = path_prefix.strip_prefix('@').unwrap_or(path_prefix);
            let (base_dir, pattern) = if let Some(last_slash) = path_part.rfind('/') {
                (&path_part[..last_slash + 1], &path_part[last_slash + 1..])
            } else {
                ("", path_part)
            };

            let base = Path::new(base_dir);
            if !base.exists() {
                return vec![];
            }

            let mut matches = vec![];
            if let Ok(entries) = std::fs::read_dir(base) {
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    let is_dir = entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false);
                    let display_name = if is_dir {
                        format!("{}/", name)
                    } else {
                        name.clone()
                    };

                    if pattern.is_empty() || display_name.starts_with(pattern) {
                        let full_path = if base_dir.is_empty() {
                            format!("@{}", display_name)
                        } else {
                            format!("@{}/{}", base_dir.trim_end_matches('/'), display_name)
                        };
                        matches.push(Suggestion {
                            value: full_path,
                            description: None,
                            extra: None,
                            span: Span::new(0, path_prefix.len()),
                            append_whitespace: is_dir,
                            ..Default::default()
                        });
                    }
                }
            }

            matches.sort_by(|a, b| a.value.cmp(&b.value));
            matches
        }
    }

    impl Completer for FilePathCompleter {
        fn complete(&mut self, line: &str, pos: usize) -> Vec<Suggestion> {
            let line = if line.len() > pos { &line[..pos] } else { line };

            // If line starts with @, do file path completion
            if line.starts_with('@') && !line.contains(' ') {
                let mut suggestions = self.complete_file_path(line);
                // If no file matches, still try default completions (tools like @file_read)
                if suggestions.is_empty() {
                    suggestions = self.default_completer.complete(line, pos);
                }
                return suggestions;
            }

            // Otherwise use default command/tool completer
            self.default_completer.complete(line, pos)
        }
    }
    let completer = FilePathCompleter::new();
    let completion_menu = ColumnarMenu::default().with_name("completion_menu");

    // Set up keybindings for Tab completion
    let mut keybindings = reedline::default_emacs_keybindings();
    keybindings.add_binding(
        KeyModifiers::NONE,
        KeyCode::Tab,
        ReedlineEvent::UntilFound(vec![
            ReedlineEvent::Menu("completion_menu".to_string()),
            ReedlineEvent::MenuNext,
        ]),
    );

    let edit_mode = Box::new(Emacs::new(keybindings));

    let mut rl = Reedline::create()
        .with_completer(Box::new(completer))
        .with_menu(ReedlineMenu::EngineCompleter(Box::new(completion_menu)))
        .with_edit_mode(edit_mode);

    // Interrupt channel for Ctrl+C during streaming
    let (interrupt_tx, mut interrupt_rx) = tokio::sync::mpsc::channel::<()>(1);
    tokio::spawn(async move {
        loop {
            tokio::signal::ctrl_c().await.ok();
            interrupt_tx.send(()).await.ok();
        }
    });

    let prompt = DefaultPrompt::default();

    loop {
        // Drain stale interrupt signals
        while interrupt_rx.try_recv().is_ok() {}

        let sig = rl.read_line(&prompt);

        match sig {
            Ok(Signal::Success(buffer)) => {
                // Drain interrupt signals after successful read
                while interrupt_rx.try_recv().is_ok() {}

                if buffer.is_empty() {
                    continue;
                }

                let input = buffer.trim().to_string();

                // Handle /save <name> command
                if input.starts_with("/save ") {
                    let name = input.trim_start_matches("/save ").trim();
                    if name.is_empty() {
                        println!("  {} Usage: /save <name>", "⚠".bright_yellow());
                    } else {
                        let data = SessionData::with_name(
                            chat_history.clone(),
                            session_usage.clone(),
                            last_reasoning.clone(),
                            name.to_string(),
                        );
                        match data.save_with_name(name) {
                            Ok(()) => {
                                current_session_name = Some(name.to_string());
                                println!(
                                    "  {} Session saved as '{}'",
                                    "💾".bright_green(),
                                    name.bright_white()
                                );
                            }
                            Err(e) => eprintln!("  {} {}", "⚠".bright_yellow(), e),
                        }
                    }
                    continue;
                }

                // Handle /sessions command
                if input == "/sessions" {
                    let sessions = SessionData::list_sessions();
                    print_sessions_list(&sessions);
                    continue;
                }

                // Handle /load <name> command
                if input.starts_with("/load ") {
                    let name = input.trim_start_matches("/load ").trim();
                    if name.is_empty() {
                        println!("  {} Usage: /load <name>", "⚠".bright_yellow());
                        println!("  {} Run /sessions to see available sessions", "→".dimmed());
                    } else {
                        match SessionData::load_by_name(name) {
                            Some(Ok(data)) => {
                                let turns = data
                                    .chat_history
                                    .iter()
                                    .filter(|m| matches!(m, rig::completion::Message::User { .. }))
                                    .count();
                                let when =
                                    my_code_agent::core::session::format_timestamp(data.saved_at);
                                chat_history = data.chat_history;
                                session_usage = data.token_usage;
                                last_reasoning = data.last_reasoning;
                                current_session_name = Some(name.to_string());
                                println!(
                                    "  {} Loaded session '{}' ({} turns from {})",
                                    "📂".bright_cyan(),
                                    name.bright_white(),
                                    turns,
                                    when.dimmed()
                                );
                            }
                            Some(Err(e)) => {
                                eprintln!(
                                    "  {} Failed to load session: {}",
                                    "⚠".bright_yellow(),
                                    e
                                );
                            }
                            None => {
                                eprintln!("  {} Session '{}' not found", "⚠".bright_yellow(), name);
                                println!(
                                    "  {} Run /sessions to see available sessions",
                                    "→".dimmed()
                                );
                            }
                        }
                    }
                    continue;
                }

                // Handle built-in commands
                if let Some(cmd) = parse_command(&input) {
                    match cmd {
                        Command::Clear => {
                            chat_history.clear();
                            last_reasoning.clear();
                            session_usage = TokenUsage::with_config(&config);
                            current_session_name = None;
                            println!("{}", "Conversation history cleared 🗑️".dimmed());
                        }
                        Command::Quit => {
                            let auto_save =
                                current_session_name.is_none() && !chat_history.is_empty();
                            if let Some(ref name) = current_session_name {
                                let data = SessionData::with_name(
                                    chat_history.clone(),
                                    session_usage.clone(),
                                    last_reasoning.clone(),
                                    name.clone(),
                                );
                                if let Err(e) = data.save_with_name(name) {
                                    eprintln!("  {} {}", "⚠".bright_yellow(), e);
                                }
                            } else if auto_save {
                                let name = "default".to_string();
                                let data = SessionData::with_name(
                                    chat_history.clone(),
                                    session_usage.clone(),
                                    last_reasoning.clone(),
                                    name.clone(),
                                );
                                if let Err(e) = data.save_with_name(&name) {
                                    eprintln!("  {} {}", "⚠".bright_yellow(), e);
                                }
                                println!("  {} Auto-saved as 'default'", "💾".bright_green());
                            }
                            println!("{}", "Goodbye! 👋".dimmed());
                            break;
                        }
                        Command::Sessions => {
                            let sessions = SessionData::list_sessions();
                            print_sessions_list(&sessions);
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

                // Use file cache for @filepath expansion
                use my_code_agent::core::context::expand_file_refs_with_cache;
                let expanded_input = if !expand_result.attachments.is_empty() {
                    let cached_result =
                        expand_file_refs_with_cache(&input, &config, Some(&mut file_cache));
                    cached_result.expanded
                } else {
                    expand_result.expanded.clone()
                };

                let result = stream_response(
                    &agent,
                    &expanded_input,
                    &mut chat_history,
                    &mut session_usage,
                    &mut interrupt_rx,
                    &mut context_manager,
                )
                .await;

                // Handle plan confirmation
                let mut plan_tracker = result.plan_tracker;
                if plan_tracker.needs_confirmation() {
                    plan_tracker.print_with_confirmation();
                    
                    // Read user confirmation
                    match rl.read_line(&DefaultPrompt::default()) {
                        Ok(Signal::Success(confirm_input)) => {
                            let confirm = confirm_input.trim().to_lowercase();
                            if confirm == "y" || confirm == "yes" || confirm.is_empty() {
                                plan_tracker.confirm();
                            } else {
                                plan_tracker.cancel();
                                println!("\n  {} Plan execution cancelled.\n", "✗".bright_red());
                                continue;
                            }
                        }
                        Ok(Signal::CtrlC) | Ok(Signal::CtrlD) => {
                            plan_tracker.cancel();
                            continue;
                        }
                        Err(_) => {
                            plan_tracker.cancel();
                            continue;
                        }
                    }
                }

                if result.should_exit {
                    let auto_save = current_session_name.is_none() && !chat_history.is_empty();
                    if let Some(ref name) = current_session_name {
                        let data = SessionData::with_name(
                            chat_history.clone(),
                            session_usage.clone(),
                            last_reasoning.clone(),
                            name.clone(),
                        );
                        if let Err(e) = data.save_with_name(name) {
                            eprintln!("  {} {}", "⚠".bright_yellow(), e);
                        }
                    } else if auto_save {
                        let name = "default".to_string();
                        let data = SessionData::with_name(
                            chat_history.clone(),
                            session_usage.clone(),
                            last_reasoning.clone(),
                            name.clone(),
                        );
                        if let Err(e) = data.save_with_name(&name) {
                            eprintln!("  {} {}", "⚠".bright_yellow(), e);
                        }
                        println!("  {} Auto-saved as 'default'", "💾".bright_green());
                    }
                    println!("{}", "Goodbye! 👋".dimmed());
                    return Ok(());
                }

                while interrupt_rx.try_recv().is_ok() {}

                last_reasoning = result.last_reasoning;

                print_interrupted_notice(&result.full_response, result.interrupted);
            }
            Ok(Signal::CtrlD) | Ok(Signal::CtrlC) => {
                let auto_save = current_session_name.is_none() && !chat_history.is_empty();
                if let Some(ref name) = current_session_name {
                    let data = SessionData::with_name(
                        chat_history.clone(),
                        session_usage.clone(),
                        last_reasoning.clone(),
                        name.clone(),
                    );
                    if let Err(e) = data.save_with_name(name) {
                        eprintln!("  {} {}", "⚠".bright_yellow(), e);
                    }
                } else if auto_save {
                    let name = "default".to_string();
                    let data = SessionData::with_name(
                        chat_history.clone(),
                        session_usage.clone(),
                        last_reasoning.clone(),
                        name.clone(),
                    );
                    if let Err(e) = data.save_with_name(&name) {
                        eprintln!("  {} {}", "⚠".bright_yellow(), e);
                    }
                    println!("  {} Auto-saved as 'default'", "💾".bright_green());
                }
                println!("{}", "Goodbye! 👋".dimmed());
                break;
            }
            Err(e) => {
                anyhow::bail!("readline error: {}", e);
            }
        }
    }

    Ok(())
}
