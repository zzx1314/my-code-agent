use crate::app::App;

pub fn handle(app: &mut App) -> bool {
    // Get the list of available sessions (latest 5)
    let sessions: Vec<_> = crate::core::session::SessionData::list_sessions()
        .into_iter()
        .take(5)
        .collect();
    if sessions.is_empty() {
        app.chat_history.push(crate::app::ChatEntry::user("/load".to_string()));
        app.chat_history.push(crate::app::ChatEntry::assistant("No saved sessions found. Use /save to save a session first.".to_string(),));
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
