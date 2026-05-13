use super::reset_input;
use super::shell::handle_shell_command;
use crate::app::App;
use crate::app::event_handler::command::handle_command;
use crate::app::event_handler::message::send_message_to_llm;
use crate::core::context_manager::ContextManager;

/// Handle Enter key press (send message)
pub fn handle_enter_key(app: &mut App, context_manager: &mut ContextManager) {
    let input_text = app.input.lines().join("\n").trim().to_string();
    if !input_text.is_empty() && !app.is_streaming {
        // Check if it's a command (starts with /)
        if input_text.starts_with('/') {
            // Handle commands locally without sending to LLM
            if handle_command(app, &input_text, context_manager) {
                reset_input(app);
                // Local commands produce non-LLM assistant messages;
                // suppress inline reasoning so it doesn't appear mispositioned
                app.show_inline_reasoning = false;
                return; // Command was handled, don't send to LLM
            }
        }

        // Shell mode: execute command in shell
        let is_shell = app.shell_mode || input_text.starts_with('!');
        if is_shell {
            handle_shell_command(app, &input_text);
            return;
        }

        // Save to input history (avoid consecutive duplicates)
        if app.input_history.last().map(|s| s.as_str()) != Some(&input_text) {
            app.input_history.push(input_text.clone());
        }
        app.history_index = None;
        app.history_draft.clear();

        send_message_to_llm(app, context_manager, input_text);
    } else if !input_text.is_empty() && app.is_streaming {
        // Save to input history (avoid consecutive duplicates)
        if app.input_history.last().map(|s| s.as_str()) != Some(&input_text) {
            app.input_history.push(input_text.clone());
        }
        app.history_index = None;
        app.history_draft.clear();

        // Queue the message for processing after current response completes
        app.message_queue.push(input_text.clone());
        app.chat_history.push(crate::app::ChatEntry::user(format!("⏳ [Queued] {}", input_text)));
        reset_input(app);
        app.auto_scroll = true;
    }
}
