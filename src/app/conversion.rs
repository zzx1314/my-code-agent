use crate::core::types::Message;

/// Convert app chat history (role, text) pairs to Message vector.
pub fn convert_app_to_messages(chat_history: &[(String, String)]) -> Vec<Message> {
    chat_history
        .iter()
        .map(|(role, content)| Message {
            role: role.clone(),
            content: content.clone(),
            reasoning_content: None,
            tool_calls: None,
            tool_call_id: None,
        })
        .collect()
}

/// Convert a Message back to a (role, text) pair.
pub fn convert_message_to_pair(msg: Message) -> (String, String) {
    (msg.role, msg.content)
}
