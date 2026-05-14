use crate::app::App;

/// Handle the `/save` command: persist the current chat session to disk.
///
/// 1. Generates a unique session name (default: timestamp-based).
/// 2. Converts the in-memory chat history into serializable `Message` structs.
/// 3. Wraps everything (history + token usage + last reasoning) into a `SessionData`.
/// 4. Saves to a file; on success, prunes old sessions keeping only the 5 newest.
/// 5. Pushes user and assistant messages into the chat so the user sees the result.
/// 6. Returns `true` to signal that the app should stay running.
pub fn handle(app: &mut App) -> bool {
    use crate::core::session::{SessionData, format_saved_confirmation, generate_session_name};
    use crate::core::types::Message;

    // --- 1. Generate a unique session identifier --------------------------------
    let session_name = generate_session_name();

    // --- 2. Extract & convert chat history into serializable Messages -----------
    let history: Vec<Message> = app
        .chat_history
        .iter()
        .map(|entry| Message {
            role: entry.role.clone(),
            content: entry.content.clone(),
            reasoning_content: entry.reasoning_content.clone(),
            tool_calls: entry.tool_calls.clone(),
            tool_call_id: entry.tool_call_id.clone(),
        })
        .collect();

    // --- 3. Build the session payload -------------------------------------------
    let data = SessionData::new(
        history,
        app.token_usage.clone(),
        app.last_reasoning.clone(),
    );

    // --- 4. Persist to disk & manage session file count -------------------------
    match data.save_with_name(&session_name) {
        Ok(()) => {
            let path = SessionData::session_file_path(&session_name);
            let msg = format_saved_confirmation(&path, &data);

            // Echo the /save command and the confirmation message back into chat
            app.chat_history.push(crate::app::ChatEntry::user("/save".to_string()));
            app.chat_history.push(crate::app::ChatEntry::assistant(msg));

            // Prune old session files, keeping only the 5 most recent ones
            if let Ok(removed) = SessionData::prune_old_sessions(5) {
                if removed > 0 {
                    tracing::info!(removed, "Pruned old session files");
                }
            }
        }
        Err(e) => {
            // Echo the /save command and an error message into chat
            app.chat_history.push(crate::app::ChatEntry::user("/save".to_string()));
            app.chat_history.push(crate::app::ChatEntry::assistant(format!(
                "❌ Failed to save session: {}",
                e,
            )));
        }
    }

    // --- 5. UI state cleanup ----------------------------------------------------
    app.show_banner = false;  // Don't show the welcome banner after a command
    app.auto_scroll = true;   // Scroll to the bottom to show the response
    true
}
