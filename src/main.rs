use anyhow::Result;
use colored::*;
use my_code_agent::core::config::Config;
use my_code_agent::core::connection::ConnectionState;
use my_code_agent::core::context::{expand_file_refs, print_attachments};
use my_code_agent::core::context_manager::ContextManager;
use my_code_agent::core::file_cache::FileCache;
use my_code_agent::core::preamble::build_agent;
use my_code_agent::core::session::{generate_session_name, search_sessions, SessionData};
use my_code_agent::core::streaming::stream_response;
use my_code_agent::core::token_usage::TokenUsage;
use my_code_agent::tools::create_mcp_tools;
use my_code_agent::ui::{
    Command, parse_command, print_banner, print_interrupted_notice, print_search_results,
    print_sessions_list, run_command,
};

use reedline::{
    ColumnarMenu, Completer, DefaultPrompt, Emacs, KeyCode, KeyModifiers, Reedline, ReedlineEvent,
    ReedlineMenu, Signal, Span, Suggestion,
};
use std::path::Path;

#[tokio::main]
async fn main() -> Result<()> {
    dotenv::dotenv().ok();

    let config = Config::load();
    print_banner();

    // Create MCP tools if enabled
    let mcp_tools = create_mcp_tools(&config).await;
    eprintln!("  {} {} MCP tools loaded", "⚙".bright_cyan(), mcp_tools.len());

    let agent = build_agent(&config, mcp_tools);

    // Initialize connection state
    let connection_state = ConnectionState::new();

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
        session_names: Vec<String>,
    }

    impl FilePathCompleter {
        fn new() -> Self {
            let commands = vec![
                // Slash commands
                "/help".into(),
                "/usage".into(),
                "/save".into(),
                "/sessions".into(),
                "/load".into(),
                "/clear".into(),
                "/new".into(),
                "/review".into(),
                "/think".into(),
                "/search".into(),
                "/quit".into(),
                "/exit".into(),
                "/q".into(),
                // At-tools (when typed manually)
                "@file_read".into(),
                "@file_write".into(),
                "@file_update".into(),
                "@file_delete".into(),
                "@shell_exec".into(),
                "@code_search".into(),
                "@code_review".into(),
                "@list_dir".into(),
                "@glob".into(),
                "@web_search".into(),
                "@web_fetch".into(),
                "@git_status".into(),
                "@git_diff".into(),
                "@git_log".into(),
                "@git_commit".into(),
            ];
            let mut default_completer =
                reedline::DefaultCompleter::with_inclusions(&['/', '@']).set_min_word_len(1);
            default_completer.insert(commands);
            Self { 
                default_completer,
                session_names: Vec::new(),
            }
        }

        /// Refresh session names from disk
        fn refresh_sessions(&mut self) {
            self.session_names = my_code_agent::core::session::SessionData::list_sessions()
                .into_iter()
                .map(|s| s.name)
                .collect();
        }

        fn complete_file_path(&self, path_prefix: &str, word_start: usize) -> Vec<Suggestion> {
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
                            format!("@{}{}", base_dir, display_name)
                        };
                        matches.push(Suggestion {
                            value: full_path,
                            description: None,
                            extra: None,
                            span: Span::new(word_start, word_start + path_prefix.len()),
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

            // Find the word being completed (after last space or from start)
            let word_start = line.rfind(' ').map(|i| i + 1).unwrap_or(0);
            let word = &line[word_start..];

            // If word starts with /, handle command completion
            if word.starts_with('/') {
                // Special handling for /load command - complete session names
                if word.starts_with("/load") {
                    self.refresh_sessions();
                    let prefix = word.strip_prefix("/load").unwrap_or("").trim();
                    let mut suggestions: Vec<Suggestion> = self.session_names
                        .iter()
                        .filter(|name| prefix.is_empty() || name.starts_with(prefix))
                        .map(|name| Suggestion {
                            value: format!("/load {}", name),
                            description: Some("session".into()),
                            extra: None,
                            span: Span::new(word_start, word_start + word.len()),
                            append_whitespace: true,
                            ..Default::default()
                        })
                        .collect();
                    suggestions.sort_by(|a, b| a.value.cmp(&b.value));
                    
                    // Also include /load command itself if just typing "/lo..."
                    if "/load".starts_with(word) || word == "/load" {
                        suggestions.push(Suggestion {
                            value: "/load ".into(),
                            description: Some("load session".into()),
                            extra: None,
                            span: Span::new(word_start, word_start + word.len()),
                            append_whitespace: false,
                            ..Default::default()
                        });
                    }
                    return suggestions;
                }
                
                // Special handling for /review command - complete file paths
                if word.starts_with("/review") {
                    let prefix = word.strip_prefix("/review").unwrap_or("").trim();
                    if !prefix.is_empty() {
                        return self.complete_file_path(&format!("@{}", prefix), word_start);
                    }
                    return vec![Suggestion {
                        value: "/review ".into(),
                        description: Some("review code at path".into()),
                        extra: None,
                        span: Span::new(word_start, word_start + word.len()),
                        append_whitespace: false,
                        ..Default::default()
                    }];
                }
                
                // Default command completion for other / commands
                return self.default_completer.complete(line, pos);
            }

            // If word starts with @, do file path completion
            if word.starts_with('@') {
                let mut suggestions = self.complete_file_path(word, word_start);
                // If no file matches, still try default completions (tools like @file_read)
                if suggestions.is_empty() {
                    suggestions = self.default_completer.complete(word, word.len());
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

    // Enable bracketed paste mode for proper multi-line paste support
    if let Err(e) = rl.enable_bracketed_paste() {
        eprintln!(
            "  {} Warning: Could not enable bracketed paste: {}",
            "⚠".bright_yellow(),
            e
        );
    }

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

        // Always display connection status
        let current_status = connection_state.get();
        eprintln!(
            "{} {}",
            current_status.emoji(),
            format!("Model: {}", current_status.text()).dimmed()
        );

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
                let mut review_prompt = None;
                if let Some(cmd) = parse_command(&input) {
                    match cmd {
                        Command::Clear => {
                            // Delete the session file if it exists and has a name
                            if let Some(ref name) = current_session_name {
                                match SessionData::delete_by_name(name) {
                                    Ok(()) => {
                                        println!(
                                            "{} Session '{}' deleted",
                                            "🗑️".bright_yellow(),
                                            name.bright_white()
                                        );
                                    }
                                    Err(e) => eprintln!("  {} {}", "⚠".bright_yellow(), e),
                                }
                            } else {
                                println!("{} No named session to delete", "ℹ️".bright_blue());
                            }
                            
                            // Clear in-memory state
                            chat_history.clear();
                            last_reasoning.clear();
                            session_usage = TokenUsage::with_config(&config);
                            current_session_name = None;
                            println!("{}", "Conversation history cleared".dimmed());
                        }
                        Command::New => {
                            // Ask user if they want to save current session
                            if !chat_history.is_empty() {
                                println!(
                                    "  {} Current session has {} turns. Save before starting new session? [y/N]",
                                    "💾".bright_yellow(),
                                    chat_history.iter().filter(|m| matches!(m, rig::completion::Message::User { .. })).count()
                                );
                                
                                // Read user input for confirmation
                                let mut input = String::new();
                                use std::io::{self, Write};
                                io::stdout().flush().ok();
                                if io::stdin().read_line(&mut input).is_ok() {
                                    let answer = input.trim().to_lowercase();
                                    if answer == "y" || answer == "yes" {
                                        // Save current session
                                        let save_name = current_session_name.clone().unwrap_or_else(|| generate_session_name());
                                        let data = SessionData::with_name(
                                            chat_history.clone(),
                                            session_usage.clone(),
                                            last_reasoning.clone(),
                                            save_name.clone(),
                                        );
                                        match data.save_with_name(&save_name) {
                                            Ok(()) => {
                                                println!(
                                                    "  {} Session '{}' saved",
                                                    "💾".bright_green(),
                                                    save_name.bright_white()
                                                );
                                            }
                                            Err(e) => eprintln!("  {} {}", "⚠".bright_yellow(), e),
                                        }
                                    }
                                }
                            }
                            
                            // Clear in-memory state to start new session
                            chat_history.clear();
                            last_reasoning.clear();
                            session_usage = TokenUsage::with_config(&config);
                            current_session_name = None;
                            println!("{}", "Started new session".bright_green());
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
                                let name = generate_session_name();
                                let data = SessionData::with_name(
                                    chat_history.clone(),
                                    session_usage.clone(),
                                    last_reasoning.clone(),
                                    name.clone(),
                                );
                                if let Err(e) = data.save_with_name(&name) {
                                    eprintln!("  {} {}", "⚠".bright_yellow(), e);
                                }
                                println!("  {} Auto-saved as '{}'", "💾".bright_green(), name.bright_white());
                            }
                            println!("{}", "Goodbye! 👋".dimmed());
                            break;
                        }
                        Command::Sessions => {
                            let sessions = SessionData::list_sessions();
                            print_sessions_list(&sessions);
                        }
                        Command::Review(path) => {
                            // Construct review prompt
                            if path.is_empty() {
                                println!("  {} Usage: /review <path>", "⚠".bright_yellow());
                                println!("  {} Example: /review src/main.rs", "→".dimmed());
                            } else {
                                review_prompt = Some(format!("请审查 {} 的代码，检查代码质量、潜在问题和改进建议", path));
                                println!("  {} Reviewing {}...", "🔍".bright_cyan(), path.bright_white());
                            }
                        }
                        Command::Search(keyword) => {
                            if keyword.is_empty() {
                                println!("  {} Usage: /search <keyword>", "⚠".bright_yellow());
                            } else {
                                let results = search_sessions(&keyword);
                                print_search_results(&results, &keyword);
                            }
                        }
                        _ => {
                            run_command(cmd, &mut session_usage, &last_reasoning, config.agent.think_command);
                        }
                    }
                    
                    // Don't continue for Review command - let it fall through to model call
                    if review_prompt.is_none() {
                        continue;
                    }
                }

                let expanded_input = if let Some(ref prompt) = review_prompt {
                    // Use review prompt directly, skip file expansion
                    prompt.clone()
                } else {
                    let expand_result = expand_file_refs(&input, &config);
                    if !expand_result.attachments.is_empty() {
                        print_attachments(&expand_result.attachments);
                    }

                    // Use file cache for @filepath expansion
                    use my_code_agent::core::context::expand_file_refs_with_cache;
                    if !expand_result.attachments.is_empty() {
                        let cached_result =
                            expand_file_refs_with_cache(&input, &config, Some(&mut file_cache));
                        cached_result.expanded
                    } else {
                        expand_result.expanded.clone()
                    }
                };

                // Update connection state to "connecting"
                connection_state.set_connecting();

                let result = stream_response(
                    &agent,
                    &expanded_input,
                    &mut chat_history,
                    &mut session_usage,
                    &mut interrupt_rx,
                    &mut context_manager,
                    &config.agent,
                )
                .await;

                // Update connection state based on result
                if result.interrupted {
                    connection_state.set_disconnected();
                    eprintln!(
                        "{} {}",
                        connection_state.get().emoji(),
                        "Model: Disconnected (interrupted)".bright_yellow()
                    );
                } else if !result.full_response.is_empty() {
                    connection_state.set_connected();
                    eprintln!(
                        "{} {}",
                        connection_state.get().emoji(),
                        "Model: Connected".bright_green()
                    );
                } else {
                    connection_state.set_error();
                    eprintln!(
                        "{} {}",
                        connection_state.get().emoji(),
                        "Model: Error (empty response)".bright_red()
                    );
                }

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
                        let name = generate_session_name();
                        let data = SessionData::with_name(
                            chat_history.clone(),
                            session_usage.clone(),
                            last_reasoning.clone(),
                            name.clone(),
                        );
                        if let Err(e) = data.save_with_name(&name) {
                            eprintln!("  {} {}", "⚠".bright_yellow(), e);
                        }
                        println!("  {} Auto-saved as '{}'", "💾".bright_green(), name.bright_white());
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
                    let name = generate_session_name();
                    let data = SessionData::with_name(
                        chat_history.clone(),
                        session_usage.clone(),
                        last_reasoning.clone(),
                        name.clone(),
                    );
                    if let Err(e) = data.save_with_name(&name) {
                        eprintln!("  {} {}", "⚠".bright_yellow(), e);
                    }
                    println!("  {} Auto-saved as '{}'", "💾".bright_green(), name.bright_white());
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
