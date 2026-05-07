use ratatui::crossterm::event::{self, KeyCode, KeyModifiers};
use std::sync::Arc;
use tui_textarea::TextArea;

use crate::app::App;
use crate::core::context_manager::ContextManager;
use super::command::handle_command;
use super::completion::{apply_completion, hide_completion, trigger_completion, update_completion_query, get_cursor_position};
use super::message::send_message_to_llm;

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
                    if let Ok(new_agent) = super::streaming::rebuild_agent(&app.config) {
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
                                .map(crate::app::conversion::convert_rig_to_app)
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
                    if let Ok(new_agent) = super::streaming::rebuild_agent(&app.config) {
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
                // Local commands produce non-LLM assistant messages;
                // suppress inline reasoning so it doesn't appear mispositioned
                app.show_inline_reasoning = false;
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
            app.show_inline_reasoning = false;
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
