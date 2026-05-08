use crate::app::App;

pub(super) fn handle(app: &mut App) -> bool {
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
