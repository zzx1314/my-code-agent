use crate::app::ChatEntry;
use crate::core::types::Message;

/// Convert app chat history entries to Message vector.
/// Preserves `reasoning_content` for DeepSeek reasoning models.
pub fn convert_app_to_messages(chat_history: &[ChatEntry]) -> Vec<Message> {
    chat_history
        .iter()
        .map(|entry| Message {
            role: entry.role.clone(),
            content: entry.content.clone(),
            reasoning_content: entry.reasoning_content.clone(),
            tool_calls: entry.tool_calls.clone(),
            tool_call_id: entry.tool_call_id.clone(),
        })
        .collect()
}
