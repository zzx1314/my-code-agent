use crate::app::App;
use crate::core::context_manager::ContextManager;

use super::streaming::{reset_streaming_state, spawn_llm_stream};
use super::init::{build_init_prompt, build_init_result, generate_knowledge_content_local};

/// Handle commands (input starting with /)
/// Returns true if the command was handled, false if it should be sent to the LLM
pub(super) fn handle_command(app: &mut App, input: &str, context_manager: &mut ContextManager) -> bool {
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
            let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel::<crate::core::streaming::StreamEvent>();
            app.streaming_events_rx = Some(event_rx);
            app.is_streaming = true;
            app.streaming_text.clear();
            app.streaming_reasoning.clear();
            app.current_tool_call = None;

            let agent_clone = app.agent.clone();
            let config_clone = app.config.clone();
            let (init_tx, init_rx) = tokio::sync::mpsc::channel::<crate::app::InitResult>(1);
            app.init_rx = Some(init_rx);

            let interrupt_rx = app.interrupt_tx.subscribe();

            tokio::spawn(async move {
                let mut chat_history = Vec::new();
                let mut token_usage = crate::core::token_usage::TokenUsage::with_config(&config_clone);
                let mut interrupt_rx = interrupt_rx;
                let mut ctx_mgr = crate::core::context_manager::ContextManager::new(&config_clone);

                let result = crate::core::streaming::stream_response(
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
                    let stripped = super::init::strip_code_fences(raw);
                    let cleaned = super::init::strip_preamble_before_heading(stripped);
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
