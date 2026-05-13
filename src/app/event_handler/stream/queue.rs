use crate::app::App;
use crate::core::context_manager::ContextManager;

use super::spawn::send_message_to_llm;

/// Process queued messages when not currently streaming
pub fn process_message_queue(app: &mut App, context_manager: &mut ContextManager) -> bool {
    if !app.is_streaming && !app.message_queue.is_empty() {
        let next_message = app.message_queue.remove(0);
        if let Some(last) = app.chat_history.last() {
            if last.content.starts_with("⏳ [Queued] ") {
                app.chat_history.pop();
            }
        }
        send_message_to_llm(app, context_manager, next_message);
        true
    } else {
        false
    }
}
