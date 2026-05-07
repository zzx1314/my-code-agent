use crossterm::{
    event::{self, KeyCode, KeyModifiers, MouseEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use std::io::Write as _;
use tokio::sync::mpsc;
use toml;
use tui_textarea::TextArea;

use crate::app::App;
use crate::app::InitResult;
use crate::app::conversion::{convert_app_to_rig, convert_rig_to_app};
use crate::core::context::expand_file_refs;
use crate::core::context_manager::ContextManager;
use crate::core::preamble::Agent;
use crate::core::streaming::{StreamEvent, StreamResult, stream_response};
use std::sync::Arc;

/// Handle key events
pub fn handle_key_event(key: event::KeyEvent, app: &mut App, context_manager: &mut ContextManager) {
    // If the confirmation dialog is showing, handle confirmation-related keys first
    if app.pending_confirmation.is_some() {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                // Confirm the action
                if let Some(pending) = app.pending_confirmation.take() {
                    let _ = pending.response_tx.send(true);
                }
                return;
            }
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                // Deny the action
                if let Some(pending) = app.pending_confirmation.take() {
                    let _ = pending.response_tx.send(false);
                }
                return;
            }
            _ => return, // Only accept y/n/Enter/Esc
        }
    }

    // If the provider picker is showing, handle provider selection keys first
    if app.show_provider_picker {
        match key.code {
            KeyCode::Down | KeyCode::Tab => {
                if !app.provider_options.is_empty() {
                    app.provider_selected =
                        (app.provider_selected + 1) % app.provider_options.len();
                }
                return;
            }
            KeyCode::Up | KeyCode::BackTab => {
                if !app.provider_options.is_empty() {
                    app.provider_selected = if app.provider_selected == 0 {
                        app.provider_options.len() - 1
                    } else {
                        app.provider_selected - 1
                    };
                }
                return;
            }
            KeyCode::Enter => {
                // Confirm provider selection
                if !app.provider_options.is_empty() {
                    let selected_provider = app.provider_options[app.provider_selected].clone();
                    app.config.llm.provider = selected_provider.clone();
                    // Update api_key_env to the default value for the selected provider
                    app.config.llm.api_key_env = match selected_provider.as_str() {
                        "deepseek" => "DEEPSEEK_API_KEY".to_string(),
                        "openrouter" => "OPENROUTER_API_KEY".to_string(),
                        _ => String::new(),
                    };
                    // Update model list to the models for the selected provider
                    app.model_options =
                        crate::app::get_model_options_for_provider(&selected_provider);
                    app.model_selected = 0;
                    // If the current model is not in the new list, reset to None
                    app.config.llm.model = app.model_options.first().cloned();

                    app.chat_history.push((
                        "user".to_string(),
                        format!("/connect {}", selected_provider),
                    ));

                    // Attempt to rebuild the agent
                    if let Ok(new_agent) = rebuild_agent(&app.config) {
                        app.agent = Arc::new(new_agent);
                        app.chat_history.push((
                            "assistant".to_string(),
                            format!(
                                "Provider switched to: {} (model: {})",
                                selected_provider,
                                app.config.llm.model.as_deref().unwrap_or("default")
                            ),
                        ));
                    } else {
                        app.chat_history.push((
                            "assistant".to_string(),
                            format!(
                                "Failed to switch provider. Please check API key and try again."
                            ),
                        ));
                    }
                }
                app.show_provider_picker = false;
                app.show_banner = false;
                app.auto_scroll = true;
                // Clear reasoning content to avoid showing the reasoning area when switching to a non-reasoning model
                app.last_reasoning.clear();
                app.streaming_reasoning.clear();
                return;
            }
            KeyCode::Esc => {
                // Cancel provider selection
                app.show_provider_picker = false;
                return;
            }
            _ => {}
        }
        return;
    }

    // If the session picker is showing, handle session selection keys first
    if app.show_session_picker {
        match key.code {
            KeyCode::Down | KeyCode::Tab => {
                if !app.session_options.is_empty() {
                    app.session_selected = (app.session_selected + 1) % app.session_options.len();
                }
                return;
            }
            KeyCode::Up | KeyCode::BackTab => {
                if !app.session_options.is_empty() {
                    app.session_selected = if app.session_selected == 0 {
                        app.session_options.len() - 1
                    } else {
                        app.session_selected - 1
                    };
                }
                return;
            }
            KeyCode::Enter => {
                // Confirm session selection
                if !app.session_options.is_empty() {
                    let selected_session = &app.session_options[app.session_selected];
                    let session_name = selected_session.name.clone();

                    // Load the selected session
                    match crate::core::session::SessionData::load_by_name(&session_name) {
                        Some(Ok(session_data)) => {
                            // Restore session data
                            app.chat_history = session_data
                                .chat_history
                                .into_iter()
                                .map(convert_rig_to_app)
                                .collect();
                            app.token_usage = session_data.token_usage;
                            app.last_reasoning = session_data.last_reasoning;

                            // Update session ID so the loaded session has its own undo context
                            let new_session_id = format!(
                                "session_{}",
                                std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_nanos()
                            );
                            crate::tools::undo_history::set_session_id(new_session_id);

                            app.chat_history
                                .push(("user".to_string(), format!("/load {}", session_name)));
                            app.chat_history.push((
                                "assistant".to_string(),
                                format!(
                                    "Session '{}' loaded ({} turns, {} tokens)",
                                    session_name, selected_session.turns, selected_session.tokens
                                ),
                            ));
                        }
                        Some(Err(e)) => {
                            app.chat_history
                                .push(("user".to_string(), format!("/load {}", session_name)));
                            app.chat_history.push((
                                "assistant".to_string(),
                                format!("Failed to load session '{}': {}", session_name, e),
                            ));
                        }
                        None => {
                            app.chat_history
                                .push(("user".to_string(), format!("/load {}", session_name)));
                            app.chat_history.push((
                                "assistant".to_string(),
                                format!("Session '{}' not found", session_name),
                            ));
                        }
                    }
                }
                app.show_session_picker = false;
                app.show_banner = false;
                app.auto_scroll = true;
                return;
            }
            KeyCode::Esc => {
                // Cancel session selection
                app.show_session_picker = false;
                return;
            }
            _ => {}
        }
        return;
    }

    // If the model picker is showing, handle model selection keys first
    if app.show_model_picker {
        match key.code {
            KeyCode::Down | KeyCode::Tab => {
                if !app.model_options.is_empty() {
                    app.model_selected = (app.model_selected + 1) % app.model_options.len();
                }
                return;
            }
            KeyCode::Up | KeyCode::BackTab => {
                if !app.model_options.is_empty() {
                    app.model_selected = if app.model_selected == 0 {
                        app.model_options.len() - 1
                    } else {
                        app.model_selected - 1
                    };
                }
                return;
            }
            KeyCode::Enter => {
                // Confirm model selection
                if !app.model_options.is_empty() {
                    let selected_model = app.model_options[app.model_selected].clone();
                    app.config.llm.model = Some(selected_model.clone());
                    app.chat_history
                        .push(("user".to_string(), format!("/model {}", selected_model)));

                    // Attempt to rebuild the agent
                    if let Ok(new_agent) = rebuild_agent(&app.config) {
                        app.agent = Arc::new(new_agent);
                        app.chat_history.push((
                            "assistant".to_string(),
                            format!("Model switched to: {}", selected_model),
                        ));
                    } else {
                        app.chat_history.push((
                            "assistant".to_string(),
                            format!("Failed to switch model. Please check API key and try again."),
                        ));
                    }
                }
                app.show_model_picker = false;
                app.show_banner = false;
                app.auto_scroll = true;
                // Clear reasoning content to avoid showing the reasoning area when switching to a non-reasoning model
                app.last_reasoning.clear();
                app.streaming_reasoning.clear();
                return;
            }
            KeyCode::Esc => {
                // Cancel model selection
                app.show_model_picker = false;
                return;
            }
            _ => {}
        }
        return;
    }

    // If the completion menu is showing, handle completion-related keys first
    if app.show_completion {
        match key.code {
            KeyCode::Down | KeyCode::Tab => {
                // Select next completion item
                if !app.completion_items.is_empty() {
                    app.completion_selected =
                        (app.completion_selected + 1) % app.completion_items.len();
                }
                return;
            }
            KeyCode::Up | KeyCode::BackTab => {
                // Select previous completion item
                if !app.completion_items.is_empty() {
                    app.completion_selected = if app.completion_selected == 0 {
                        app.completion_items.len() - 1
                    } else {
                        app.completion_selected - 1
                    };
                }
                return;
            }
            KeyCode::Enter => {
                // Confirm completion
                let is_command_completion = app.completion_type == Some('/');
                apply_completion(app);
                // If it's a command completion, execute the command directly (no need to press Enter again)
                if is_command_completion {
                    handle_enter_key(app, context_manager);
                }
                return;
            }
            KeyCode::Esc => {
                // Cancel completion
                hide_completion(app);
                return;
            }
            KeyCode::Char(c) => {
                // Input character, update completion query
                if c == '/' && app.completion_type != Some('/') {
                    // Switch to command completion
                    hide_completion(app);
                    trigger_completion(app, '/');
                    return;
                } else if c == '@' && app.completion_type != Some('@') {
                    // Switch to file completion
                    hide_completion(app);
                    trigger_completion(app, '@');
                    return;
                }
                // Other characters: let the input box handle it, then update completion
            }
            KeyCode::Backspace => {
                // Backspace: check if completion should be hidden
            }
            _ => {}
        }
    }

    match (key.code, key.modifiers) {
        (KeyCode::Char('c'), modifiers) if modifiers.contains(KeyModifiers::CONTROL) => {
            // Ctrl+C when streaming: handled by the broadcast interrupt
            if !app.is_streaming {
                app.should_exit = true;
            }
        }
        (KeyCode::Char('r'), modifiers) if modifiers.contains(KeyModifiers::CONTROL) => {
            // Ctrl+R: toggle reasoning display
            app.show_reasoning = !app.show_reasoning;
        }
        (KeyCode::Esc, _) => {
            if app.show_completion {
                hide_completion(app);
            } else if app.is_streaming {
                let _ = app.interrupt_tx.send(());
                app.response_rx = None;
                app.streaming_events_rx = None;
                app.init_rx = None;
                app.is_streaming = false;
                app.streaming_text.clear();
                app.streaming_reasoning.clear();
                app.current_tool_call = None;
                app.status_messages.clear();
                app.streaming_status_messages.clear();
            } else {
                app.should_exit = true;
            }
        }
        (KeyCode::Enter, modifiers) => {
            if modifiers.contains(KeyModifiers::ALT) {
                // Alt+Enter: insert newline in textarea
                app.input.input(key);
            } else {
                // Plain Enter or Ctrl+Enter: send
                if app.show_completion {
                    let is_command_completion = app.completion_type == Some('/');
                    apply_completion(app);
                    // If it's a command completion, execute the command directly
                    if is_command_completion {
                        handle_enter_key(app, context_manager);
                    }
                } else {
                    handle_enter_key(app, context_manager);
                }
            }
        }
        (KeyCode::PageUp, _) => {
            app.scroll = app.scroll.saturating_sub(3);
            app.auto_scroll = false;
        }
        (KeyCode::PageDown, _) => {
            let max_scroll = app.total_lines.saturating_sub(app.chat_area_height);
            app.scroll = (app.scroll + 3).min(max_scroll);
            app.auto_scroll = false;
        }
        (KeyCode::Up, modifiers) if modifiers.is_empty() => {
            if app.show_completion {
                // Select previous completion item
                if !app.completion_items.is_empty() {
                    app.completion_selected = if app.completion_selected == 0 {
                        app.completion_items.len() - 1
                    } else {
                        app.completion_selected - 1
                    };
                }
            } else {
                app.scroll = app.scroll.saturating_sub(3);
                app.auto_scroll = false;
            }
        }
        (KeyCode::Down, modifiers) if modifiers.is_empty() => {
            if app.show_completion {
                // Select next completion item
                if !app.completion_items.is_empty() {
                    app.completion_selected =
                        (app.completion_selected + 1) % app.completion_items.len();
                }
            } else {
                let max_scroll = app.total_lines.saturating_sub(app.chat_area_height);
                app.scroll = (app.scroll + 3).min(max_scroll);
                if app.scroll >= max_scroll {
                    app.auto_scroll = true;
                }
            }
        }
        (KeyCode::Char(c), _) => {
            // Check if completion should be triggered
            if c == '/' || c == '@' {
                app.input.input(key);
                trigger_completion(app, c);
            } else {
                app.input.input(key);
                // If the completion menu is showing, update the filter
                if app.show_completion {
                    update_completion_query(app);
                }
            }
        }
        (KeyCode::Backspace, _) => {
            app.input.input(key);
            // Check if completion needs to be hidden or updated
            if app.show_completion {
                let cursor_pos = get_cursor_position(app);
                // If the character before the cursor is not '/' or '@', or the cursor is before the trigger position, hide completion
                if cursor_pos == 0 || (cursor_pos <= app.completion_trigger_pos) {
                    hide_completion(app);
                } else {
                    update_completion_query(app);
                }
            }
        }
        _ => {
            app.input.input(key);
        }
    }
}

/// Handle Enter key press (send message)
fn handle_enter_key(app: &mut App, context_manager: &mut ContextManager) {
    let input_text = app.input.lines().join("\n").trim().to_string();
    if !input_text.is_empty() && !app.is_streaming {
        // Check if it's a command (starts with /)
        if input_text.starts_with('/') {
            // Handle commands locally without sending to LLM
            if handle_command(app, &input_text, context_manager) {
                // Clear input after command is handled
                let title = if app.shell_mode {
                    " Input 🐚 Shell Mode (Enter to exec, !exit to leave, /shell to toggle) "
                } else {
                    " Input (Enter to send, Alt+Enter for newline, Esc: interrupt/exit) "
                };
                app.input = {
                    let mut ta = TextArea::default();
                    ta.set_block(
                        ratatui::widgets::Block::default()
                            .borders(ratatui::widgets::Borders::ALL)
                            .title(title),
                    );
                    ta.set_cursor_line_style(ratatui::style::Style::default());
                    ta
                };
                return; // Command was handled, don't send to LLM
            }
        }

        // Shell mode: execute command in shell
        let is_shell = app.shell_mode || input_text.starts_with('!');
        if is_shell {
            let cmd = if app.shell_mode {
                // Shell mode: entire input is the command
                input_text.clone()
            } else {
                // ! prefix: strip the '!' and execute
                input_text
                    .strip_prefix('!')
                    .unwrap_or(&input_text)
                    .trim()
                    .to_string()
            };

            // Handle shell mode exit commands
            if cmd == "exit" || cmd == "!exit" {
                app.shell_mode = false;
                app.chat_history
                    .push(("user".to_string(), input_text.clone()));
                app.chat_history.push((
                    "assistant".to_string(),
                    "🐚 Shell mode deactivated.".to_string(),
                ));
                app.input = {
                    let mut ta = TextArea::default();
                    ta.set_block(
                        ratatui::widgets::Block::default()
                            .borders(ratatui::widgets::Borders::ALL)
                            .title(" Input (Enter to send, Alt+Enter for newline, Esc: interrupt/exit) ")
                    );
                    ta.set_cursor_line_style(ratatui::style::Style::default());
                    ta
                };
                app.show_banner = false;
                app.auto_scroll = true;
                return;
            }

            if cmd.is_empty() {
                return;
            }

            // Handle cd command specially — subprocess cd doesn't affect the parent process
            let cmd_trimmed = cmd.trim();
            let is_cd = cmd_trimmed == "cd"
                || cmd_trimmed.starts_with("cd ")
                || cmd_trimmed.starts_with("cd\t");
            if is_cd {
                let target = if cmd_trimmed == "cd" {
                    // cd with no args goes to HOME
                    std::env::var("HOME").unwrap_or_else(|_| "/".to_string())
                } else {
                    cmd_trimmed[2..].trim().to_string()
                };
                // Handle ~ expansion
                let target = if target.starts_with('~') {
                    if let Ok(home) = std::env::var("HOME") {
                        target.replacen('~', &home, 1)
                    } else {
                        target
                    }
                } else {
                    target
                };
                app.chat_history
                    .push(("user".to_string(), format!("$ {}", cmd)));
                match std::env::set_current_dir(&target) {
                    Ok(()) => {
                        let cwd = std::env::current_dir()
                            .map(|p| p.display().to_string())
                            .unwrap_or_else(|_| "?".to_string());
                        app.chat_history.push((
                            "assistant".to_string(),
                            format!("Changed directory to {}", cwd),
                        ));
                    }
                    Err(e) => {
                        app.chat_history
                            .push(("assistant".to_string(), format!("❌ cd: {}: {}", target, e)));
                    }
                }
                let title = if app.shell_mode {
                    " Input 🐚 Shell Mode (Enter to exec, !exit to leave, /shell to toggle) "
                } else {
                    " Input (Enter to send, Alt+Enter for newline, Esc: interrupt/exit) "
                };
                app.input = {
                    let mut ta = TextArea::default();
                    ta.set_block(
                        ratatui::widgets::Block::default()
                            .borders(ratatui::widgets::Borders::ALL)
                            .title(title),
                    );
                    ta.set_cursor_line_style(ratatui::style::Style::default());
                    ta
                };
                app.show_banner = false;
                app.auto_scroll = true;
                return;
            }

            app.chat_history.push((
                "user".to_string(),
                if app.shell_mode {
                    format!("$ {}", cmd)
                } else {
                    input_text.clone()
                },
            ));

            // Execute shell command
            let output = std::process::Command::new("bash")
                .arg("-c")
                .arg(&cmd)
                .current_dir(
                    std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
                )
                .output();

            match output {
                Ok(o) => {
                    let stdout = String::from_utf8_lossy(&o.stdout);
                    let stderr = String::from_utf8_lossy(&o.stderr);
                    let mut result = String::new();
                    if !stdout.is_empty() {
                        result.push_str(&stdout);
                    }
                    if !stderr.is_empty() {
                        if !result.is_empty() {
                            result.push('\n');
                        }
                        result.push_str(&format!("stderr:\n{}", stderr));
                    }
                    if !o.status.success() {
                        result
                            .push_str(&format!("\n(exit code: {})", o.status.code().unwrap_or(-1)));
                    }
                    if result.is_empty() {
                        result = "(no output)".to_string();
                    }
                    app.chat_history.push(("assistant".to_string(), result));
                }
                Err(e) => {
                    app.chat_history.push((
                        "assistant".to_string(),
                        format!("❌ Failed to execute command: {}", e),
                    ));
                }
            }

            let title = if app.shell_mode {
                " Input 🐚 Shell Mode (Enter to exec, !exit to leave, /shell to toggle) "
            } else {
                " Input (Enter to send, Alt+Enter for newline, Esc: interrupt/exit) "
            };
            app.input = {
                let mut ta = TextArea::default();
                ta.set_block(
                    ratatui::widgets::Block::default()
                        .borders(ratatui::widgets::Borders::ALL)
                        .title(title),
                );
                ta.set_cursor_line_style(ratatui::style::Style::default());
                ta
            };
            app.show_banner = false;
            app.auto_scroll = true;
            return;
        }

        send_message_to_llm(app, context_manager, input_text);
    } else if !input_text.is_empty() && app.is_streaming {
        // Queue the message for processing after current response completes
        app.message_queue.push(input_text.clone());
        app.chat_history
            .push(("user".to_string(), format!("⏳ [Queued] {}", input_text)));
        // Clear input
        let title = if app.shell_mode {
            " Input 🐚 Shell Mode (Enter to exec, !exit to leave, /shell to toggle) "
        } else {
            " Input (Enter to send, Alt+Enter for newline, Esc: interrupt/exit) "
        };
        app.input = {
            let mut ta = TextArea::default();
            ta.set_block(
                ratatui::widgets::Block::default()
                    .borders(ratatui::widgets::Borders::ALL)
                    .title(title),
            );
            ta.set_cursor_line_style(ratatui::style::Style::default());
            ta
        };
        app.auto_scroll = true;
    }
}

/// Reset all streaming-related state in the app
fn reset_streaming_state(app: &mut App) {
    app.is_streaming = true;
    app.auto_scroll = true;
    app.reasoning_auto_scroll = true;
    app.reasoning_scroll = 0;
    app.streaming_text.clear();
    app.streaming_reasoning.clear();
    app.current_tool_call = None;
    app.current_response.clear();
    app.status_messages.clear();
    app.streaming_status_messages.clear();
    app.turn_usage_line = None;
}

/// Spawn an async LLM streaming task with the given prompt
fn spawn_llm_stream(app: &mut App, context_manager: &mut ContextManager, prompt: &str) {
    let expanded = expand_file_refs(prompt, &app.config);

    let mut rig_chat_history = convert_app_to_rig(&app.chat_history);
    let agent_clone = app.agent.clone();
    let config_clone = app.config.clone();
    let mut token_usage_clone = app.token_usage.clone();
    let interrupt_rx = app.interrupt_tx.subscribe();
    let (response_tx, response_rx) = mpsc::channel::<StreamResult>(1);
    let (event_tx, event_rx) = mpsc::unbounded_channel::<StreamEvent>();

    let mut ctx_mgr = context_manager.clone();

    app.response_rx = Some(response_rx);
    app.streaming_events_rx = Some(event_rx);

    tokio::spawn(async move {
        let mut interrupt_rx = interrupt_rx;

        let result = stream_response(
            &agent_clone,
            &expanded.expanded,
            &mut rig_chat_history,
            &mut token_usage_clone,
            &mut interrupt_rx,
            &mut ctx_mgr,
            &config_clone.agent,
            Some(event_tx),
        )
        .await;

        response_tx.send(result).await.ok();
    });
}

/// Send a message to the LLM (extracted for reuse by message queue)
pub fn send_message_to_llm(
    app: &mut App,
    context_manager: &mut ContextManager,
    input_text: String,
) {
    app.show_banner = false; // Hide startup banner
    app.chat_history
        .push(("user".to_string(), input_text.clone()));
    app.input = {
        let mut ta = TextArea::default();
        ta.set_block(
            ratatui::widgets::Block::default()
                .borders(ratatui::widgets::Borders::ALL)
                .title(" Input (Enter to send, Alt+Enter for newline, Esc: interrupt/exit) "),
        );
        ta.set_cursor_line_style(ratatui::style::Style::default());
        ta
    };
    reset_streaming_state(app);
    spawn_llm_stream(app, context_manager, &input_text);
}

pub fn handle_paste_event(text: &str, app: &mut App) {
    for ch in text.chars() {
        if ch == '\n' || ch == '\r' {
            let key = event::KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT);
            app.input.input(key);
        } else {
            let key = event::KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE);
            app.input.input(key);
        }
    }
}

/// Handle mouse events
pub fn handle_mouse_event(mouse: event::MouseEvent, app: &mut App) {
    match mouse.kind {
        MouseEventKind::ScrollUp => {
            app.scroll = app.scroll.saturating_sub(3);
            app.auto_scroll = false;
        }
        MouseEventKind::ScrollDown => {
            let max_scroll = app.total_lines.saturating_sub(app.chat_area_height);
            app.scroll = (app.scroll + 3).min(max_scroll);
            // Re-enable auto_scroll when scrolled to the bottom
            if app.scroll >= max_scroll {
                app.auto_scroll = true;
            }
        }
        _ => {} // Ignore other mouse events without affecting text selection
    }
}

/// Process streaming events
pub fn process_streaming_events(app: &mut App) {
    // Poll streaming text events for live display
    if let Some(ref mut rx) = app.streaming_events_rx {
        loop {
            match rx.try_recv() {
                Ok(StreamEvent::Text(delta)) => {
                    if app.current_tool_call.is_some() {
                        app.streaming_text.push_str("\n\n");
                    }
                    app.streaming_text.push_str(&delta);
                    app.current_tool_call = None;
                }
                Ok(StreamEvent::ToolCall(name)) => {
                    app.current_tool_call = Some(name);
                }
                Ok(StreamEvent::ReasoningActive(active)) => {
                    if !active {
                        // Reasoning ended, save content to last_reasoning
                        if !app.streaming_reasoning.is_empty() {
                            app.last_reasoning = app.streaming_reasoning.clone();
                            app.streaming_reasoning.clear();
                        }
                    }
                }
                Ok(StreamEvent::ReasoningDelta(delta)) => {
                    app.streaming_reasoning.push_str(&delta);
                    app.current_tool_call = None;
                }
                Ok(StreamEvent::PlanProgress(msg)) => {
                    app.streaming_status_messages.push(msg);
                }
                Err(mpsc::error::TryRecvError::Empty) => break,
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    app.streaming_events_rx = None;
                    break;
                }
            }
        }
    }
}

/// Check for completed stream result
pub fn check_stream_result(app: &mut App) {
    if let Some(ref mut rx) = app.response_rx {
        match rx.try_recv() {
            Ok(result) => {
                // Only process results while still in streaming state
                // If already force-cleaned by Esc (is_streaming=false), discard old results
                if app.is_streaming {
                    process_stream_result(app, result);
                }
                app.response_rx = None;
            }
            Err(mpsc::error::TryRecvError::Disconnected) => {
                if app.is_streaming {
                    cleanup_stream_state(app);
                }
                app.response_rx = None;
            }
            Err(mpsc::error::TryRecvError::Empty) => {}
        }
    }
}

fn cleanup_stream_state(app: &mut App) {
    app.is_streaming = false;
    app.streaming_text.clear();
    app.streaming_reasoning.clear();
    app.current_tool_call = None;
    app.streaming_events_rx = None;
    app.streaming_status_messages.clear();
    app.auto_scroll = true;
}

pub fn check_init_result(app: &mut App) {
    if let Some(ref mut rx) = app.init_rx {
        match rx.try_recv() {
            Ok(result) => {
                app.chat_history
                    .push(("assistant".to_string(), result.message));
                if let Some(new_agent) = result.new_agent {
                    app.agent = Arc::new(new_agent);
                }
                app.init_rx = None;
                // Clean up all streaming state
                app.is_streaming = false;
                app.streaming_text.clear();
                app.streaming_reasoning.clear();
                app.current_tool_call = None;
                app.streaming_events_rx = None;
                app.streaming_status_messages.clear();
                app.auto_scroll = true;
                app.scroll = u16::MAX;
            }
            Err(mpsc::error::TryRecvError::Empty) => {}
            Err(mpsc::error::TryRecvError::Disconnected) => {
                app.init_rx = None;
                app.is_streaming = false;
                app.streaming_text.clear();
                app.streaming_reasoning.clear();
                app.current_tool_call = None;
                app.streaming_events_rx = None;
                app.streaming_status_messages.clear();
                app.auto_scroll = true;
            }
        }
    }
}

/// Process stream result
fn process_stream_result(app: &mut App, result: StreamResult) {
    app.is_streaming = false;
    app.streaming_text.clear();
    app.streaming_reasoning.clear();
    app.current_tool_call = None;
    app.streaming_events_rx = None;
    app.streaming_status_messages.clear();

    if !result.full_response.is_empty() || (!result.last_reasoning.is_empty() && app.config.agent.thinking_display != "hidden") {
        let mut combined = String::new();

        if app.config.agent.thinking_display != "hidden" && !result.last_reasoning.is_empty() {
            let reasoning_block = format!("> {}", result.last_reasoning.replace('\n', "\n> "));
            combined.push_str(&reasoning_block);
            combined.push_str("\n\n");
        }

        combined.push_str(&result.full_response);
        app.chat_history.push(("assistant".to_string(), combined));
    }

    app.token_usage = result.session_usage;
    app.status_messages = result.status_messages;
    app.turn_usage_line = result.turn_usage_line;
    app.auto_scroll = true;

    if result.should_exit {
        app.should_exit = true;
    }
    }

/// Process queued messages after streaming completes.
/// Returns true if there was a queued message and it started sending.
pub fn process_message_queue(app: &mut App, context_manager: &mut ContextManager) -> bool {
    if !app.is_streaming && !app.message_queue.is_empty() {
        let next_message = app.message_queue.remove(0);
        // Remove the "[Queued]" entry from chat history since we're now sending the real message
        if let Some(last) = app.chat_history.last() {
            if last.1.starts_with("⏳ [Queued] ") {
                app.chat_history.pop();
            }
        }
        send_message_to_llm(app, context_manager, next_message);
        true
    } else {
        false
    }
}

/// Enter alternate screen and enable raw mode
pub fn enter_terminal() -> anyhow::Result<Terminal<CrosstermBackend<std::io::Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let _ = write!(std::io::stdout(), "\x1b[?1007h");
    let _ = write!(std::io::stdout(), "\x1b[?2004h");
    let _ = std::io::stdout().flush();
    let backend = CrosstermBackend::new(stdout);
    let terminal = Terminal::new(backend)?;
    Ok(terminal)
}

/// Leave alternate screen and disable raw mode
pub fn leave_terminal(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
) -> anyhow::Result<()> {
    let _ = write!(std::io::stdout(), "\x1b[?1007l");
    let _ = write!(std::io::stdout(), "\x1b[?2004l");
    let _ = std::io::stdout().flush();
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    Ok(())
}

// ========== Completion menu utility functions ==========

/// Trigger the completion menu
fn trigger_completion(app: &mut App, trigger_char: char) {
    app.show_completion = true;
    app.completion_type = Some(trigger_char);
    app.completion_selected = 0;
    app.completion_trigger_pos = get_cursor_position(app);
    app.completion_query = String::new();

    // Fetch completion items
    app.completion_items = get_completion_items(trigger_char);
}

/// Hide the completion menu
fn hide_completion(app: &mut App) {
    app.show_completion = false;
    app.completion_items.clear();
    app.completion_selected = 0;
    app.completion_type = None;
    app.completion_query.clear();
    app.completion_trigger_pos = 0;
}

/// Convert a character index to a byte index (handles multi-byte characters)
fn char_idx_to_byte(s: &str, char_idx: usize) -> usize {
    s.char_indices()
        .nth(char_idx)
        .map(|(byte_idx, _)| byte_idx)
        .unwrap_or(s.len())
}

/// Apply the selected completion
fn apply_completion(app: &mut App) {
    if app.completion_items.is_empty() {
        hide_completion(app);
        return;
    }

    let selected = app.completion_items[app.completion_selected].clone();
    let trigger_char = match app.completion_type {
        Some(c) => c,
        None => {
            hide_completion(app);
            return;
        }
    };

    // Get the current input text
    let mut lines: Vec<String> = app.input.lines().iter().map(|s| s.to_string()).collect();
    let cursor = app.input.cursor();

    // Find the current line
    if cursor.0 < lines.len() {
        let line = &mut lines[cursor.0];
        let pos = char_idx_to_byte(line, cursor.1);

        // Find the position of the trigger character (search backwards from the cursor position)
        let trigger_pos = line[..pos]
            .rfind(trigger_char)
            .unwrap_or(pos.saturating_sub(1));

        // Replace content from the trigger position to the cursor position
        let new_line = format!("{}{}{}", &line[..trigger_pos], selected, &line[pos..]);
        lines[cursor.0] = new_line;
    }

    let new_text = lines.join("\n");
    let mut new_input = TextArea::from(new_text.lines());
    new_input.set_block(
        ratatui::widgets::Block::default()
            .borders(ratatui::widgets::Borders::ALL)
            .title(" Input (Enter to send, Alt+Enter for newline, Esc: interrupt/exit) "),
    );
    new_input.set_cursor_line_style(ratatui::style::Style::default());
    app.input = new_input;

    // Set cursor position to the end of the completion
    let completion_len = selected.len();
    let cursor = app.input.cursor();
    let new_cursor_col = app.completion_trigger_pos + completion_len;
    app.input.move_cursor(tui_textarea::CursorMove::Jump(
        cursor.0 as u16,
        new_cursor_col as u16,
    ));

    hide_completion(app);
}

/// Update the completion query string (used for filtering)
fn update_completion_query(app: &mut App) {
    let cursor_pos = get_cursor_position(app);
    if cursor_pos <= app.completion_trigger_pos {
        hide_completion(app);
        return;
    }

    // Get text from trigger position to cursor position as the query string
    let lines: Vec<String> = app.input.lines().iter().map(|s| s.to_string()).collect();
    let cursor = app.input.cursor();

    if cursor.0 < lines.len() {
        let line = &lines[cursor.0];
        let byte_start = char_idx_to_byte(line, app.completion_trigger_pos);
        let byte_end = char_idx_to_byte(line, cursor_pos);
        if byte_start <= byte_end && byte_end <= line.len() {
            app.completion_query = line[byte_start..byte_end].to_string();
        }
    }

    // Filter completion items
    if let Some(trigger_char) = app.completion_type {
        let all_items = get_completion_items(trigger_char);
        if app.completion_query.is_empty() {
            app.completion_items = all_items;
        } else {
            app.completion_items = all_items
                .into_iter()
                .filter(|item| {
                    item.to_lowercase()
                        .contains(&app.completion_query.to_lowercase())
                })
                .collect();
        }
        app.completion_selected = 0;
    }
}

/// Get the current cursor position (character offset)
fn get_cursor_position(app: &App) -> usize {
    let cursor = app.input.cursor();
    cursor.1
}

/// Get the list of completion items
fn get_completion_items(trigger_char: char) -> Vec<String> {
    match trigger_char {
        '/' => {
            // Command completion
            vec![
                "/help".to_string(),
                "/quit".to_string(),
                "/clear".to_string(),
                "/save".to_string(),
                "/load".to_string(),
                "/status".to_string(),
                "/tokens".to_string(),
                "/think".to_string(),
                "/connect".to_string(),
                "/model".to_string(),
                "/init".to_string(),
                "/undo".to_string(),
                "/plan".to_string(),
                "/shell".to_string(),
            ]
        }
        '@' => {
            // File completion - use glob to get files in the current directory
            use glob::glob;
            let mut files = Vec::new();

            // Get all files in the current directory (recursive depth 2)
            if let Ok(entries) = glob("**/*") {
                for entry in entries.flatten() {
                    if let Some(path_str) = entry.to_str() {
                        // Skip hidden files and directories
                        if !path_str.starts_with('.') && !path_str.contains("/.") {
                            files.push(format!("@{}", path_str));
                        }
                    }
                }
            }

            // If no files found, add some examples
            if files.is_empty() {
                files.push("@src/main.rs".to_string());
                files.push("@src/lib.rs".to_string());
                files.push("@Cargo.toml".to_string());
                files.push("@README.md".to_string());
            }

            files.sort();
            files.dedup();
            files
        }
        _ => Vec::new(),
    }
}

/// Handle commands (input starting with /)
/// Returns true if the command was handled, false if it should be sent to the LLM
fn handle_command(app: &mut App, input: &str, context_manager: &mut ContextManager) -> bool {
    let command = input.trim().to_lowercase();

    match command.as_str() {
        "/help" => {
            let help_text = generate_help_text();
            app.chat_history
                .push(("user".to_string(), "/help".to_string()));
            app.chat_history.push(("assistant".to_string(), help_text));
            app.show_banner = false;
            app.auto_scroll = true;
            app.scroll = u16::MAX;
            true
        }
        "/quit" => {
            app.should_exit = true;
            true
        }
        "/clear" => {
            app.chat_history.clear();
            app.token_usage = crate::core::token_usage::TokenUsage::with_config(&app.config);
            app.show_banner = true;
            app.auto_scroll = true;
            app.scroll = 0;
            // Delete session file
            if app.config.session.enabled {
                if let Some(save_file) = &app.config.session.save_file {
                    let _ = std::fs::remove_file(save_file);
                }
            }
            // Clear undo history for current session
            if let Err(e) = crate::tools::undo_history::clear_current_session_entries() {
                tracing::warn!(error = %e, "Failed to clear undo history on /clear");
            }
            // Generate a new session ID so the cleared session's history is separate
            let new_session_id = format!(
                "session_{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos()
            );
            crate::tools::undo_history::set_session_id(new_session_id);
            true
        }
        "/save" => {
            use crate::core::session::{
                SessionData, format_saved_confirmation, generate_session_name,
            };

            let session_name = generate_session_name();
            let rig_history: Vec<_> = app
                .chat_history
                .iter()
                .map(|(r, c)| match r.as_str() {
                    "user" => rig::completion::Message::user(c.clone()),
                    "assistant" => rig::completion::Message::assistant(c.clone()),
                    _ => rig::completion::Message::user(c.clone()),
                })
                .collect();

            let data = SessionData::new(
                rig_history,
                app.token_usage.clone(),
                app.last_reasoning.clone(),
            );

            match data.save_with_name(&session_name) {
                Ok(()) => {
                    let path = SessionData::session_file_path(&session_name);
                    let msg = format_saved_confirmation(&path, &data);
                    app.chat_history
                        .push(("user".to_string(), "/save".to_string()));
                    app.chat_history.push(("assistant".to_string(), msg));

                    // Prune old sessions, keeping only the 5 newest
                    if let Ok(removed) = SessionData::prune_old_sessions(5) {
                        if removed > 0 {
                            tracing::info!(removed, "Pruned old session files");
                        }
                    }
                }
                Err(e) => {
                    app.chat_history
                        .push(("user".to_string(), "/save".to_string()));
                    app.chat_history.push((
                        "assistant".to_string(),
                        format!("❌ Failed to save session: {}", e),
                    ));
                }
            }

            app.show_banner = false;
            app.auto_scroll = true;
            true
        }
        "/load" => {
            // Get the list of available sessions (latest 5)
            let sessions: Vec<_> = crate::core::session::SessionData::list_sessions()
                .into_iter()
                .take(5)
                .collect();
            if sessions.is_empty() {
                app.chat_history
                    .push(("user".to_string(), "/load".to_string()));
                app.chat_history.push((
                    "assistant".to_string(),
                    "No saved sessions found. Use /save to save a session first.".to_string(),
                ));
                app.show_banner = false;
                app.auto_scroll = true;
            } else {
                // Show session picker
                app.session_options = sessions;
                app.session_selected = 0;
                app.show_session_picker = true;
            }
            true
        }
        "/status" => {
            let status = format!(
                "Session enabled: {}\nModel: {}\nProvider: {}\nTotal tokens used: {}",
                app.config.session.enabled,
                app.config.llm.model.as_deref().unwrap_or("default"),
                app.config.llm.provider,
                app.token_usage.total_tokens()
            );
            app.chat_history
                .push(("user".to_string(), "/status".to_string()));
            app.chat_history.push(("assistant".to_string(), status));
            app.show_banner = false;
            app.auto_scroll = true;
            true
        }
        "/tokens" => {
            let mut report = app.token_usage.format_session_report();
            // Append session-wide cache metrics
            let cache_report = crate::core::context_cache::global_cache().format_session_report();
            report.extend(cache_report);
            let token_info = report.join("\n").trim().to_string();
            app.chat_history
                .push(("user".to_string(), "/tokens".to_string()));
            app.chat_history.push(("assistant".to_string(), token_info));
            app.show_banner = false;
            app.auto_scroll = true;
            true
        }
        "/connect" => {
            // Open provider picker
            app.show_provider_picker = true;
            // Find the position of the current provider in the options
            if let Some(pos) = app
                .provider_options
                .iter()
                .position(|p| p == &app.config.llm.provider)
            {
                app.provider_selected = pos;
            }
            true
        }
        "/think" => {
            app.chat_history
                .push(("user".to_string(), "/think".to_string()));

            if !app.last_reasoning.is_empty() {
                app.chat_history.push(("assistant".to_string(), format!("💭 Reasoning:\n─────────────────────────────────────────\n{}\n─────────────────────────────────────────", app.last_reasoning)));
            } else if !app.streaming_reasoning.is_empty() {
                app.chat_history.push(("assistant".to_string(), format!("💭 Thinking (in progress):\n─────────────────────────────────────────\n{}\n─────────────────────────────────────────", app.streaming_reasoning)));
            } else {
                app.chat_history.push(("assistant".to_string(), "No reasoning available. Reasoning is only available when using a model that supports thinking (e.g., deepseek-reasoner), and will be shown after the model responds.".to_string()));
            }

            app.show_banner = false;
            app.auto_scroll = true;
            true
        }
        "/model" => {
            // Open model picker, ensuring the model list corresponds to the current provider
            app.model_options =
                crate::app::get_model_options_for_provider(&app.config.llm.provider);
            app.show_model_picker = true;
            // Find the position of the current model in the options
            if let Some(current_model) = &app.config.llm.model {
                if let Some(pos) = app.model_options.iter().position(|m| m == current_model) {
                    app.model_selected = pos;
                }
            }
            true
        }
        "/init" => {
            let knowledge_file = crate::core::preamble::KNOWLEDGE_FILE.to_string();
            let is_update = std::path::Path::new(&knowledge_file).exists();
            let prompt = build_init_prompt(is_update);

            app.chat_history
                .push(("user".to_string(), "/init".to_string()));
            app.chat_history.push((
                "assistant".to_string(),
                if is_update {
                    "⏳ Updating knowledge file — exploring project..."
                } else {
                    "⏳ Creating knowledge file — exploring project..."
                }
                .to_string(),
            ));
            app.show_banner = false;
            app.auto_scroll = true;
            app.scroll = u16::MAX;

            // Set up streaming channels
            let (event_tx, event_rx) = mpsc::unbounded_channel::<StreamEvent>();
            app.streaming_events_rx = Some(event_rx);
            app.is_streaming = true;
            app.streaming_text.clear();
            app.streaming_reasoning.clear();
            app.current_tool_call = None;

            let agent_clone = app.agent.clone();
            let config_clone = app.config.clone();
            let (init_tx, init_rx) = mpsc::channel::<InitResult>(1);
            app.init_rx = Some(init_rx);

            let interrupt_rx = app.interrupt_tx.subscribe();

            tokio::spawn(async move {
                let mut chat_history = Vec::new();
                let mut token_usage = crate::core::token_usage::TokenUsage::with_config(&config_clone);
                let mut interrupt_rx = interrupt_rx;
                let mut ctx_mgr = crate::core::context_manager::ContextManager::new(&config_clone);

                let result = stream_response(
                    &agent_clone,
                    &prompt,
                    &mut chat_history,
                    &mut token_usage,
                    &mut interrupt_rx,
                    &mut ctx_mgr,
                    &config_clone.agent,
                    Some(event_tx),
                )
                .await;

                // Extract content: use LLM response, or fallback to local generation
                let new_content = if result.full_response.is_empty() {
                    tracing::warn!(
                        "LLM returned empty response for /init, falling back to local generation"
                    );
                    generate_knowledge_content_local()
                } else {
                    let raw = result.full_response.trim();
                    let stripped = strip_code_fences(raw);
                    let cleaned = strip_preamble_before_heading(stripped);
                    tracing::info!(
                        bytes = cleaned.len(),
                        "Generated knowledge content via LLM"
                    );
                    cleaned.to_string()
                };

                let init_result =
                    build_init_result(&knowledge_file, &new_content, &config_clone, is_update);
                init_tx.send(init_result).await.ok();
            });

            true
        }
        "/undo" => {
            use crate::tools::file_undo;
            use crate::tools::undo_history::{
                current_session_history_len, pop_current_session_entries,
            };

            app.chat_history
                .push(("user".to_string(), "/undo".to_string()));
            app.show_banner = false;
            app.auto_scroll = true;

            let available = current_session_history_len().unwrap_or(0);
            if available == 0 {
                app.chat_history.push(("assistant".to_string(), "No undo history for current session. Undo history is recorded when AI tools modify files during this session.".to_string()));
            } else {
                match pop_current_session_entries() {
                    Ok(entries) if entries.is_empty() => {
                        app.chat_history.push((
                            "assistant".to_string(),
                            "No undo history for current session.".to_string(),
                        ));
                    }
                    Ok(entries) => {
                        let mut details = Vec::new();
                        let mut errors = Vec::new();
                        for entry in &entries {
                            if let Err(e) = file_undo::apply_undo(entry, &mut details) {
                                errors.push(e.to_string());
                            }
                        }
                        let mut msg = format!(
                            "↩️ Undid {} change(s) for current session:\n",
                            details.len()
                        );
                        for d in &details {
                            msg.push_str(&format!(
                                "  • `{}`: {} ({})\n",
                                d.file_path, d.action, d.operation
                            ));
                        }
                        if !errors.is_empty() {
                            msg.push_str(&format!("\n⚠️ Errors:\n"));
                            for e in &errors {
                                msg.push_str(&format!("  • {}\n", e));
                            }
                        }
                        app.chat_history.push(("assistant".to_string(), msg));
                    }
                    Err(e) => {
                        app.chat_history
                            .push(("assistant".to_string(), format!("❌ Undo failed: {}", e)));
                    }
                }
            }
            true
        }
        "/shell" => {
            app.shell_mode = !app.shell_mode;
            app.chat_history
                .push(("user".to_string(), "/shell".to_string()));
            if app.shell_mode {
                app.chat_history.push(("assistant".to_string(), "🐚 Shell mode activated! All input will be executed as shell commands.\nType `exit` or `/shell` to deactivate.".to_string()));
            } else {
                app.chat_history.push((
                    "assistant".to_string(),
                    "🐚 Shell mode deactivated.".to_string(),
                ));
            }
            app.show_banner = false;
            app.auto_scroll = true;
            true
        }
        cmd if cmd.starts_with("/plan") => handle_plan_command(app, input, context_manager),
        _ => {
            // Unknown command, send to the LLM for handling
            false
        }
    }
}

/// Handle the /plan command: analyze task and create implementation plan without executing
fn handle_plan_command(app: &mut App, input: &str, context_manager: &mut ContextManager) -> bool {
    let task = input.trim().strip_prefix("/plan").unwrap_or("").trim();
    app.chat_history
        .push(("user".to_string(), input.to_string()));
    app.show_banner = false;

    if task.is_empty() {
        app.chat_history.push((
            "assistant".to_string(),
            "📋 **Plan Mode**\n\n\
                    Usage: `/plan <task description>`\n\n\
                    Example: `/plan Add user authentication with JWT tokens`\n\n\
                    In plan mode, I will analyze your task and create a detailed plan \
                    without executing any actions. You can review the plan before proceeding."
                .to_string(),
        ));
        app.auto_scroll = true;
        return true;
    }

    let planning_prompt = format!(
        r#"You are in PLAN-ONLY mode. Your task is to analyze the following request and create a comprehensive, actionable plan.

## Rules for Plan Mode:
- Do NOT execute any tools (no file reads, writes, shell commands, etc.)
- Focus ONLY on planning and analysis
- Create a detailed, step-by-step implementation plan
- Identify potential risks, dependencies, and prerequisites
- Estimate complexity for each step
- Suggest a logical execution order

## Output Format:
Structure your plan as follows:

### 🎯 Objective
[Clear summary of what needs to be accomplished]

### 📋 Prerequisites
[Any setup, dependencies, or information needed before starting]

### 📝 Implementation Plan
1. **Step 1: [Action]**
   - Details: [What exactly to do]
   - Files affected: [Which files to create/modify]
   - Complexity: [Low/Medium/High]
   
2. **Step 2: [Action]**
   ...

### ⚠️ Risks & Considerations
[Potential issues, edge cases, or things to watch out for]

### ✅ Success Criteria
[How to verify the task is complete]

---
**Task:** {task}"#
    );

    reset_streaming_state(app);
    spawn_llm_stream(app, context_manager, &planning_prompt);

    true
}

/// Generate help text
fn generate_help_text() -> String {
    let help = r#"# My Code Agent - Command Help

## Available Commands

| Command | Description |
|---------|-------------|
| `/help` | Show this help message |
| `/quit` | Exit the application |
| `/clear` | Clear chat history and start fresh |
| `/save` | Save session (auto-saves on exit) |
| `/load` | Load/resume a saved session |
| `/status` | Show current configuration and status |
| `/tokens` | Show token usage statistics |
| `/connect` | Select LLM provider (deepseek / openrouter) |
| `/model` | Select model from dropdown menu |
| `/think` | Show last reasoning/thinking content |
| `/init` | Initialize or update project knowledge file |
| `/undo` | Undo all file changes made in this session (restore to session start) |
| `/plan <task>` | Enter plan mode — analyze and create an implementation plan without executing |
| `/shell` | Toggle shell mode (all input executed as shell commands) |

## Input Features

- **`@filepath`** - Attach a file inline (e.g., `@src/main.rs`)
  - Use `@path:N` to read from line N (e.g., `@src/main.rs:50`)
  - Large files (>500 lines or 50KB) are truncated with a notice

- **`!command`** - Execute a shell command directly (e.g., `!ls -la`)
- **`/shell`** - Enter persistent shell mode (type `exit` or `/shell` to leave)
- **Alt+Enter** - Insert newline in input
- **Enter** - Send message
- **Esc** / **Ctrl+C** - Interrupt response | **Esc** twice / **Ctrl+C** twice - Quit
- **Ctrl+R** - Toggle reasoning display
- **PageUp/PageDown** - Scroll chat history
- **Mouse wheel** - Scroll chat history

## Tools Available (13 total)

`file_read` · `file_write` · `file_update` · `file_delete` · `shell_exec` · `code_search` · `code_review` · `list_dir` · `glob` · `git_status` · `git_diff` · `git_log` · `git_commit`

## Tips

- Type your question or task in natural language
- Attach files using `@filepath` for context
- The AI will automatically use tools when needed
- Sessions auto-save to `.session.json` if enabled in config.toml
"#;
    help.to_string()
}

/// Rebuild the agent (used for model switching)
/// Build the LLM prompt for `/init` command
fn build_init_prompt(is_update: bool) -> String {
    if is_update {
        let existing_content =
            std::fs::read_to_string(crate::core::preamble::KNOWLEDGE_FILE).unwrap_or_default();
        format!(
            r#"You are a technical documentation expert. Your task is to UPDATE the project knowledge document.

Current knowledge document:
```markdown
{}
```

## Instructions
1. Use the available tools (list_dir, glob, file_read, code_search) to explore the current project structure
2. Check the project root for README.md, Cargo.toml, package.json, or similar config files
3. Look at the source directory structure to understand the codebase layout
4. Update the knowledge document to accurately reflect the current state of the project
5. Keep the existing Markdown structure but update all content
6. Add any new important files, modules, or patterns you discover
7. Remove references to files or features that no longer exist

## Output Rules
- Respond ONLY with the complete updated Markdown content
- Do NOT include any explanation, commentary, or wrapping text
- Do NOT use code fences around your response
- The response should be valid Markdown that can be directly saved as a file"#,
            existing_content
        )
    } else {
        r#"You are a technical documentation expert. Your task is to CREATE a comprehensive project knowledge document.

## Instructions
1. Use the available tools (list_dir, glob, file_read, code_search) to thoroughly explore the project
2. Read README.md, Cargo.toml/package.json, and other config files in the project root
3. Explore the source directory structure (src/, lib/, etc.)
4. Identify the project type, key dependencies, architecture patterns, and conventions
5. Create a well-structured Markdown knowledge document

## Document Structure
The document should include these sections:
- **## What This Is** — Brief project description (from README or code)
- **## Features** — Key features and capabilities
- **## Project Structure** — Directory/file layout with descriptions
- **## Key Dependencies** — Major libraries and their purposes
- **## Configuration** — How the project is configured
- **## Conventions & Gotchas** — Important patterns, naming conventions, things to know

## Output Rules
- Respond ONLY with the complete Markdown content
- Do NOT include any explanation, commentary, or wrapping text
- Do NOT use code fences around your response
- The response should be valid Markdown that can be directly saved as a file"#.to_string()
    }
}

/// Strip wrapping code fences from LLM response (e.g. ```markdown ... ```)
fn strip_code_fences(raw: &str) -> &str {
    if raw.starts_with("```") && raw.ends_with("```") {
        let inner = &raw[raw.find('\n').unwrap_or(3)..raw.len() - 3];
        inner.trim()
    } else {
        raw
    }
}

/// Strip any preamble text before the first Markdown heading (#).
/// LLMs sometimes prepend explanatory text like "Here is the knowledge document:"
/// before the actual content. This function finds the first line starting with `#`
/// and removes everything before it.
fn strip_preamble_before_heading(raw: &str) -> &str {
    for (i, line) in raw.lines().enumerate() {
        if line.starts_with('#') {
            // Found the first heading — return from here
            let offset: usize = raw.lines().take(i).map(|l| l.len() + 1).sum();
            return &raw[offset..];
        }
        // Keep looking only through non-empty preamble lines;
        // if we hit a non-empty, non-heading line followed by content, still continue
        // until we find a heading or exhaust the search
    }
    // No heading found — return as-is
    raw
}

/// Write knowledge content to disk and rebuild the agent, returning an `InitResult`
fn build_init_result(
    knowledge_file: &str,
    new_content: &str,
    config: &crate::core::config::Config,
    is_update: bool,
) -> InitResult {
    let action = if is_update { "Updated" } else { "Created" };
    match std::fs::write(knowledge_file, new_content) {
        Ok(_) => match rebuild_agent(config) {
            Ok(new_agent) => InitResult {
                message: format!(
                    "✅ {} '{}' ({} bytes) with current project info.\nAgent reloaded with updated knowledge.",
                    action,
                    knowledge_file,
                    new_content.len()
                ),
                new_agent: Some(new_agent),
            },
            Err(e) => InitResult {
                message: format!(
                    "✅ {} '{}' with current project info.\n⚠️ Failed to reload agent: {}",
                    action, knowledge_file, e
                ),
                new_agent: None,
            },
        },
        Err(e) => InitResult {
            message: format!("❌ Failed to write '{}': {}", knowledge_file, e),
            new_agent: None,
        },
    }
}

fn rebuild_agent(config: &crate::core::config::Config) -> anyhow::Result<Agent> {
    use crate::core::preamble::build_agent;
    use crate::tools::create_mcp_tools;

    let mcp_tools = futures::executor::block_on(create_mcp_tools(config));
    Ok(build_agent(config, mcp_tools))
}

/// Local fallback for knowledge generation (no LLM)
fn generate_knowledge_content_local() -> String {
    let mut content = String::new();
    content.push_str("# Project Knowledge\n\n");

    // === What This Is ===
    content.push_str("## What This Is\n");
    if let Ok(readme) = std::fs::read_to_string("README.md") {
        // Try to extract the first meaningful paragraph after the title
        let meaningful: String = readme
            .lines()
            .skip_while(|line| line.starts_with('#') || line.trim().is_empty())
            .take_while(|line| !line.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        if !meaningful.is_empty() {
            content.push_str(&meaningful);
            content.push_str("\n\n");
        } else {
            content.push_str("[Project description from README.md]\n\n");
        }
    } else {
        content.push_str("[Describe your project here]\n\n");
    }

    // === Project Structure ===
    content.push_str("## Project Structure\n\n");
    content.push_str("```\n");
    if let Ok(entries) = glob::glob("**/*.rs") {
        let mut files: Vec<String> = entries
            .filter_map(|e| e.ok())
            .filter(|e| !e.to_string_lossy().contains("target/"))
            .map(|e| e.to_string_lossy().to_string())
            .collect();
        files.sort();
        files.dedup();
        for file in files.iter().take(40) {
            content.push_str(&format!("{}\n", file));
        }
        if files.len() > 40 {
            content.push_str(&format!("... ({} more files)\n", files.len() - 40));
        }
    }
    content.push_str("```\n\n");

    // === Entry Points ===
    content.push_str("## Entry Points\n\n");
    let entry_files = ["src/main.rs", "src/lib.rs", "src/index.rs", "src/app.rs"];
    for entry in &entry_files {
        if std::path::Path::new(entry).exists() {
            content.push_str(&format!("- `{}`\n", entry));
        }
    }
    content.push_str("\n");

    // === Key Dependencies ===
    content.push_str("## Key Dependencies\n\n");
    if let Ok(cargo_content) = std::fs::read_to_string("Cargo.toml") {
        if let Ok(cargo_toml) = cargo_content.parse::<toml::Value>() {
            // Main dependencies
            if let Some(deps) = cargo_toml.get("dependencies").and_then(|d| d.as_table()) {
                let mut dep_list: Vec<String> = deps
                    .iter()
                    .filter(|(name, _)| !name.starts_with('_')) // skip internal path deps
                    .map(|(name, value)| {
                        let version = match value {
                            toml::Value::String(v) => v.clone(),
                            toml::Value::Table(t) => {
                                let ver = t.get("version").and_then(|v| v.as_str()).unwrap_or("*");
                                let features = t
                                    .get("features")
                                    .and_then(|f| f.as_array())
                                    .map(|arr| {
                                        let feats: Vec<&str> =
                                            arr.iter().filter_map(|v| v.as_str()).collect();
                                        format!(" (features: {})", feats.join(", "))
                                    })
                                    .unwrap_or_default();
                                format!("{}{}", ver, features)
                            }
                            _ => "*".to_string(),
                        };
                        format!("- **{}** v{}", name, version)
                    })
                    .collect();
                dep_list.sort();
                content.push_str(&dep_list.join("\n"));
                content.push_str("\n\n");
            }
            // Dev dependencies
            if let Some(dev_deps) = cargo_toml
                .get("dev-dependencies")
                .and_then(|d| d.as_table())
            {
                if !dev_deps.is_empty() {
                    content.push_str("### Dev Dependencies\n\n");
                    let mut dev_list: Vec<String> = dev_deps
                        .iter()
                        .map(|(name, value)| {
                            let version = match value {
                                toml::Value::String(v) => v.clone(),
                                _ => "*".to_string(),
                            };
                            format!("- **{}** v{}", name, version)
                        })
                        .collect();
                    dev_list.sort();
                    content.push_str(&dev_list.join("\n"));
                    content.push_str("\n\n");
                }
            }
        }
    }

    // === Rust Edition & Features ===
    if let Ok(cargo_content) = std::fs::read_to_string("Cargo.toml") {
        if let Ok(cargo_toml) = cargo_content.parse::<toml::Value>() {
            if let Some(package) = cargo_toml.get("package") {
                let mut meta_parts = Vec::new();
                if let Some(edition) = package.get("edition").and_then(|e| e.as_str()) {
                    meta_parts.push(format!("Rust edition: {}", edition));
                }
                if let Some(name) = package.get("name").and_then(|n| n.as_str()) {
                    meta_parts.push(format!("Crate name: {}", name));
                }
                if !meta_parts.is_empty() {
                    content.push_str("## Project Metadata\n\n");
                    for part in &meta_parts {
                        content.push_str(&format!("- {}\n", part));
                    }
                    content.push_str("\n");
                }
            }
        }
    }

    // === Test Files ===
    content.push_str("## Tests\n\n");
    if let Ok(entries) = glob::glob("tests/**/*.rs") {
        let mut test_files: Vec<String> = entries
            .filter_map(|e| e.ok())
            .map(|e| e.to_string_lossy().to_string())
            .collect();
        test_files.sort();
        if test_files.is_empty() {
            content.push_str("[No test files found in tests/]\n\n");
        } else {
            for file in test_files.iter().take(15) {
                content.push_str(&format!("- `{}`\n", file));
            }
            if test_files.len() > 15 {
                content.push_str(&format!(
                    "... ({} more test files)\n",
                    test_files.len() - 15
                ));
            }
            content.push_str("\n");
        }
    }

    // === Conventions ===
    content.push_str("## Conventions & Gotchas\n\n");
    // Auto-detect some conventions
    let mut conventions = Vec::new();
    if std::path::Path::new(".gitignore").exists() {
        conventions.push("- Project uses .gitignore for version control");
    }
    if std::path::Path::new("clippy.toml").exists() || std::path::Path::new("rustfmt.toml").exists()
    {
        conventions.push("- Clippy/rustfmt configuration present — follow formatting rules");
    }
    if std::path::Path::new(".github/workflows").exists() {
        conventions.push("- CI/CD workflows in `.github/workflows/`");
    }
    if conventions.is_empty() {
        conventions.push("- [Add important conventions here]");
    }
    for conv in &conventions {
        content.push_str(&format!("{}\n", conv));
    }

    content
}
