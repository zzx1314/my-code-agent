use crate::app::App;

pub(super) fn handle(app: &mut App) -> bool {
    app.shell_mode = !app.shell_mode;
    app.chat_history.push(crate::app::ChatEntry::user("/shell".to_string()));
    if app.shell_mode {
        app.chat_history.push(crate::app::ChatEntry::assistant("🐚 Shell mode activated! All input will be executed as shell commands.\nType `exit` or `/shell` to deactivate.".to_string()));
    } else {
        app.chat_history.push(crate::app::ChatEntry::assistant("🐚 Shell mode deactivated.".to_string(),));
    }
    app.show_banner = false;
    app.auto_scroll = true;
    true
}
