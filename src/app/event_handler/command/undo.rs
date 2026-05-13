use crate::app::App;

pub(super) fn handle(app: &mut App) -> bool {
    use crate::tools::file_undo;
    use crate::tools::undo_history::{current_session_history_len, pop_current_session_entries};

    app.chat_history.push(crate::app::ChatEntry::user("/undo".to_string()));
    app.show_banner = false;
    app.auto_scroll = true;

    let available = current_session_history_len().unwrap_or(0);
    if available == 0 {
        app.chat_history.push(crate::app::ChatEntry::assistant("No undo history for current session. Undo history is recorded when AI tools modify files during this session.".to_string()));
    } else {
        match pop_current_session_entries() {
            Ok(entries) if entries.is_empty() => {
                app.chat_history.push(crate::app::ChatEntry::assistant("No undo history for current session.".to_string(),));
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
                    msg.push_str("\n⚠️ Errors:\n");
                    for e in &errors {
                        msg.push_str(&format!("  • {}\n", e));
                    }
                }
                app.chat_history.push(crate::app::ChatEntry::assistant(msg));
            }
            Err(e) => {
                app.chat_history.push(crate::app::ChatEntry::assistant(format!("❌ Undo failed: {}", e)));
            }
        }
    }
    true
}
