use crate::core::context::context_manager::ContextManager;
use crate::core::types::Message;

/// Extension methods for [`ContextManager`] used during stream processing.
impl ContextManager {
    /// Remove the oldest conversation round (user + assistant pair) when the
    /// message list exceeds the token budget.
    pub fn trim(&self, messages: &mut Vec<Message>) {
        while self.estimate_messages_tokens(messages, false) > self.estimate_max_tokens() {
            Self::drop_oldest_round(messages);
        }
    }

    /// Drop the earliest user–assistant round, including any trailing tool
    /// messages that belong to the removed assistant turn.
    pub(super) fn drop_oldest_round(messages: &mut Vec<Message>) {
        if let Some(pos) = messages.iter().position(|m| m.role == "user") {
            let end = messages[pos + 1..]
                .iter()
                .position(|m| m.role == "user")
                .map(|i| pos + 1 + i)
                .unwrap_or(pos + 2);
            // Extend past any tool messages that belong to the removed
            // assistant, to avoid leaving orphaned tool messages which
            // DeepSeek rejects.
            let mut actual_end = end.min(messages.len());
            while actual_end < messages.len() && messages[actual_end].role == "tool" {
                actual_end += 1;
            }
            messages.drain(pos..actual_end);
        }
    }
}
