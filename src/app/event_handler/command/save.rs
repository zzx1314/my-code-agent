use crate::app::App;

pub(super) fn handle(app: &mut App) -> bool {
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
