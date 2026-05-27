//! Review request and event types
//!
//! Data structures for initiating code reviews and streaming review progress events.

/// Review Request
pub struct ReviewRequest {
    pub changed_files: Vec<crate::core::types::review::ChangedFile>,
    pub context: Option<String>,         // Original task description
    pub history_summary: Option<String>, // Conversation history summary for consistency checking
}

/// Review response events
#[derive(Debug, Clone)]
pub enum ReviewEvent {
    Started {
        file_count: usize,
    },
    Progress {
        message: String,
    },
    /// Reasoning/thinking content from the LLM during review.
    /// Displayed on the frontend but NOT added to conversation history.
    ReasoningDelta(String),
    /// Streaming natural language review feedback content.
    ReviewFeedbackDelta(String),
    Completed {
        report: crate::core::types::review::ReviewReport,
    },
    Error {
        message: String,
    },
}
