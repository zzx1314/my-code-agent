use crate::app::App;

pub(super) fn handle(app: &mut App) -> bool {
    use crate::core::session::{SessionData, format_saved_confirmation, generate_session_name};
    use crate::core::types::Message;

    let session_name = generate_session_name();
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

    let data = SessionData::new(
        history,
        app.token_usage.clone(),
        app.last_reasoning.clone(),
    );

    match data.save_with_name(&session_name) {
        Ok(()) => {
            let path = SessionData::session_file_path(&session_name);
            let msg = format_saved_confirmation(&path, &data);
            app.chat_history.push(crate::app::ChatEntry::user("/save".to_string()));
            app.chat_history.push(crate::app::ChatEntry::assistant(msg));

            // Prune old sessions, keeping only the 5 newest
            if let Ok(removed) = SessionData::prune_old_sessions(5) {
                if removed > 0 {
                    tracing::info!(removed, "Pruned old session files");
                }
            }
        }
        Err(e) => {
            app.chat_history.push(crate::app::ChatEntry::user("/save".to_string()));
            app.chat_history.push(crate::app::ChatEntry::assistant(format!("❌ Failed to save session: {}", e),));
        }
    }

    app.show_banner = false;
    app.auto_scroll = true;
    true
}
