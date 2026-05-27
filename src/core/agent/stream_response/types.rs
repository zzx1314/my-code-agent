use crate::core::context::token_usage::TokenUsage;
use crate::core::types::Message;

/// Public result type returned from [`stream_response`].
#[derive(Clone)]
pub struct StreamResult {
    pub full_response: String,
    pub interrupted: bool,
    pub should_exit: bool,
    pub last_reasoning: String,
    pub status_messages: Vec<String>,
    pub turn_usage_line: Option<String>,
    pub session_usage: TokenUsage,
    pub updated_history: Vec<Message>,
}

#[derive(Debug, Clone)]
pub enum StreamEvent {
    Text(String),
    ToolCall {
        name: String,
        arguments: String,
    },
    ToolResult {
        name: String,
        content: String,
    },
    /// Status update for inter-turn waiting periods (e.g. "Waiting for model...")
    /// Clears the previous streaming_tool_result so the UI doesn't render heavy content.
    Status(String),
    ReasoningActive(bool),
    ReasoningDelta(String),
}
