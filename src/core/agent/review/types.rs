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
    FileAnalyzed {
        file: String,
        issues_found: usize,
    },
    Progress {
        message: String,
    },
    /// Emitted when a review phase completes (used for phased/multi-category review)
    PhaseCompleted {
        phase_index: usize,      // 1-based phase number
        total_phases: usize,     // total number of phases
        phase_name: String,      // e.g. "Core Correctness"
        categories: Vec<String>, // category names checked in this phase
        issues_found: usize,     // number of issues found
        passed: bool,            // true if no issues
        details: String,         // brief summary
    },
    /// Reasoning/thinking content from the LLM during review.
    /// Displayed on the frontend but NOT added to conversation history.
    ReasoningDelta(String),
    Completed {
        report: crate::core::types::review::ReviewReport,
    },
    Error {
        message: String,
    },
}
