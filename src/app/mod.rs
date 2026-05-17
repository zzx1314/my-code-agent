use crate::core::config::Config;
use crate::core::agent::preamble::Agent;
use crate::core::agent::stream_response::{StreamEvent, StreamResult};
use crate::core::context::token_usage::TokenUsage;
use crate::tools::exec::confirmation::ConfirmationRequest;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc;
use tui_textarea::TextArea;

// App initialization & project knowledge (/init command)
pub mod bootstrap;
pub mod commands;
pub mod terminal;

/// Represents a pending confirmation request from a tool.
pub struct PendingConfirmation {
    /// The reason for the confirmation (e.g. "dangerous command")
    pub reason: String,
    /// Detailed description of the action
    pub detail: String,
    /// The sender to respond with the user's decision
    pub response_tx: tokio::sync::oneshot::Sender<bool>,
}

/// Result from async /init command
/// Represents a tool call currently being executed during streaming.
pub struct CurrentToolCall {
    pub name: String,
    pub arguments: String,
}

pub struct InitResult {
    pub message: String,
    pub new_agent: Option<Agent>,
}

pub mod event_handler;
pub mod lifecycle;

/// A single entry in the chat history, preserving reasoning content and tool
/// metadata for DeepSeek reasoning models across user turns.
#[derive(Debug, Clone)]
pub struct ChatEntry {
    pub role: String,
    pub content: String,
    pub reasoning_content: Option<String>,
    pub tool_calls: Option<Vec<crate::core::types::ToolCall>>,
    pub tool_call_id: Option<String>,
}

impl ChatEntry {
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".into(),
            content: content.into(),
            reasoning_content: None,
            tool_calls: None,
            tool_call_id: None,
        }
    }

    #[allow(dead_code)]
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: "assistant".into(),
            content: content.into(),
            reasoning_content: None,
            tool_calls: None,
            tool_call_id: None,
        }
    }

    pub fn assistant_with_reasoning(content: impl Into<String>, reasoning: impl Into<String>) -> Self {
        let r = reasoning.into();
        Self {
            role: "assistant".into(),
            content: content.into(),
            reasoning_content: if r.is_empty() { None } else { Some(r) },
            tool_calls: None,
            tool_call_id: None,
        }
    }

    /// Convert from a core `Message`, preserving all fields.
    pub fn from_message(m: crate::core::types::Message) -> Self {
        Self {
            role: m.role,
            content: m.content,
            reasoning_content: m.reasoning_content,
            tool_calls: m.tool_calls,
            tool_call_id: m.tool_call_id,
        }
    }
}

/// Application state
pub struct App {
    pub chat_history: Vec<ChatEntry>,
    pub current_response: String,
    pub input: TextArea<'static>,
    pub scroll: u16,
    pub total_lines: u16,
    pub token_usage: TokenUsage,
    pub last_reasoning: String,
    pub config: Config,
    pub should_exit: bool,
    pub is_streaming: bool,
    pub response_rx: Option<mpsc::Receiver<StreamResult>>,
    pub streaming_events_rx: Option<mpsc::UnboundedReceiver<StreamEvent>>,
    pub streaming_text: String,
    pub streaming_reasoning: String,
    /// Whether the model is actively producing reasoning in the current response.
    /// Set by ReasoningActive events during streaming. Prevents showing the thinking
    /// area for models that don't produce reasoning output.
    pub is_reasoning_active: bool,
    /// Currently executing tool call (displayed inline, replaces previous)
    pub current_tool_call: Option<CurrentToolCall>,
    /// Most recent completed tool result during streaming (tool name, content)
    pub streaming_tool_result: Option<(String, String)>,
    /// Status message for inter-turn waiting periods (e.g. "⏳ Waiting for model...")
    pub streaming_status: String,
    /// Persistent todos display during streaming (survives single-frame `.take()` on tool result)
    pub streaming_todos: Option<String>,
    pub status_messages: Vec<String>,
    pub turn_usage_line: Option<String>,
    /// Agent for processing requests
    pub agent: Arc<Agent>,
    /// Broadcast channel for interrupt signals
    pub interrupt_tx: tokio::sync::broadcast::Sender<()>,
    /// Whether to show the reasoning area
    pub show_reasoning: bool,
    /// Scroll position within the reasoning area
    pub reasoning_scroll: u16,
    /// Total number of lines in the reasoning area
    pub reasoning_total_lines: u16,
    /// Whether to automatically scroll to the latest content
    pub auto_scroll: bool,
    /// Whether the reasoning area auto-scrolls
    pub reasoning_auto_scroll: bool,
    /// Whether to show the banner at startup (hidden after the first message is sent)
    pub show_banner: bool,
    /// Marquee animation frame counter
    pub marquee_frame: u64,
    // === Completion menu state ===
    /// Whether to show the completion menu
    pub show_completion: bool,
    /// Completion item list (filtered view)
    pub completion_items: Vec<String>,
    /// Full unfiltered completion item list (cached on trigger to avoid expensive re-computation)
    pub completion_all_items: Vec<String>,
    /// Index of the currently selected completion item
    pub completion_selected: usize,
    /// Completion type: '/' command completion or '@' file completion
    pub completion_type: Option<char>,
    /// Completion query string (used for filtering)
    pub completion_query: String,
    /// Completion trigger position (cursor position)
    pub completion_trigger_pos: usize,
    /// Chat area height
    pub chat_area_height: u16,
    // === Confirmation dialog state ===
    /// Currently pending confirmation request
    pub pending_confirmation: Option<PendingConfirmation>,
    // === Model picker state ===
    /// Whether to show the model picker
    pub show_model_picker: bool,
    /// Available model options
    pub model_options: Vec<String>,
    /// Index of the currently selected model
    pub model_selected: usize,
    // === Provider picker state ===
    /// Whether to show the provider picker
    pub show_provider_picker: bool,
    /// Available provider options
    pub provider_options: Vec<String>,
    /// Index of the currently selected provider
    pub provider_selected: usize,
    // === Session picker state ===
    /// Whether to show the session picker
    pub show_session_picker: bool,
    /// Available session options
    pub session_options: Vec<crate::core::session::SessionInfo>,
    /// Index of the currently selected session
    pub session_selected: usize,
    pub init_rx: Option<mpsc::Receiver<InitResult>>,
    /// Receiver for confirmation requests from tools
    pub confirmation_rx: Option<tokio::sync::mpsc::UnboundedReceiver<ConfirmationRequest>>,
    /// Whether Shell mode is active (all input is executed as shell commands)
    pub shell_mode: bool,
    /// Message queue: messages entered by the user while the model is still streaming are queued here
    pub message_queue: Vec<String>,
    /// Whether to render reasoning inline (before the last LLM assistant message).
    /// Set to true when an LLM response with reasoning completes, false when a local
    /// command pushes a non-LLM assistant message.
    pub show_inline_reasoning: bool,
    /// Cursor animation start time — drives smooth breathing via wall-clock elapsed.
    pub cursor_anim_start: Instant,
    // === Input history ===
    /// History of previously sent input texts (newest last)
    pub input_history: Vec<String>,
    /// Current position in input history while navigating; None = not browsing history.
    /// Index 0 is the oldest entry, last index is the newest.
    pub history_index: Option<usize>,
    /// Draft text saved when the user starts browsing history (so we can restore it on Down past the end)
    pub history_draft: String,
    // === Collapsible sections state ===
    /// Track which sections are collapsed (section_id -> collapsed)
    pub collapsed_sections: std::collections::HashSet<String>,
    /// Toggle positions for mouse click handling: (logical_line_index, section_id, content_line_count)
    pub collapsed_toggles: Vec<(u16, String, usize)>,
    // === Code Review state ===
    /// Agent orchestrator for multi-agent coordination
    pub orchestrator: Option<std::sync::Arc<crate::core::agent::orchestrator::AgentOrchestrator>>,
    /// Whether a review is in progress
    pub is_reviewing: bool,
    /// Receiver for review events (progress updates)
    pub review_event_rx: Option<tokio::sync::mpsc::UnboundedReceiver<crate::app::commands::review::ReviewEvent>>,
    /// Receiver for review result
    pub review_result_rx: Option<tokio::sync::mpsc::Receiver<crate::core::types::review::ReviewOutcome>>,
    /// Current auto-review iteration count (0 = first review cycle)
    pub review_iteration: usize,
    /// Transient message to show in status bar after review completes
    pub review_complete_message: Option<String>,
    /// Verdict of the completed review (for color-coding the status bar message)
    pub review_complete_verdict: Option<crate::core::types::review::ReviewVerdict>,
    /// Frames remaining to show review_complete_message (~10 frames/second)
    pub review_complete_timer: u64,
    /// Reasoning content from the review agent's LLM calls.
    /// Displayed on the frontend but NOT added to conversation history.
    pub review_reasoning: String,
    /// Issues from the previous auto-review iteration.
    /// Used for fingerprint-based deduplication to prevent repeated false positives.
    /// Cleared when the review loop ends (approved or max iterations reached).
    pub previous_review_issues: Vec<crate::core::types::review::ReviewIssue>,
}

impl App {
    /// Create a new App instance
    pub fn new(
        chat_history: Vec<ChatEntry>,
        token_usage: TokenUsage,
        last_reasoning: String,
        config: Config,
        agent: Arc<Agent>,
        interrupt_tx: tokio::sync::broadcast::Sender<()>,
    ) -> Self {
        let show_banner = chat_history.is_empty();
        let mut input_area = TextArea::default();
        // Initial block is a placeholder; update_input_style() in ui() sets the real style.
        input_area.set_block(
            ratatui::widgets::Block::default()
                .borders(ratatui::widgets::Borders::ALL)
                .border_type(ratatui::widgets::BorderType::Double),
        );
        input_area.set_cursor_line_style(ratatui::style::Style::default());

        App {
            chat_history,
            current_response: String::new(),
            streaming_tool_result: None,
            streaming_status: String::new(),
            streaming_todos: None,
            input: input_area,
            scroll: 0,
            total_lines: 0,
            token_usage,
            last_reasoning,
            config: config.clone(),
            should_exit: false,
            is_streaming: false,
            response_rx: None,
            streaming_events_rx: None,
            streaming_text: String::new(),
            streaming_reasoning: String::new(),
            is_reasoning_active: false,
            current_tool_call: None,
            status_messages: Vec::new(),
            turn_usage_line: None,
            agent,
            interrupt_tx,
            show_reasoning: true,
            reasoning_scroll: 0,
            reasoning_total_lines: 0,
            auto_scroll: true,
            reasoning_auto_scroll: true,
            show_banner,
            marquee_frame: 0,
            // Completion menu initialization
            show_completion: false,
            completion_items: Vec::new(),
            completion_all_items: Vec::new(),
            completion_selected: 0,
            completion_type: None,
            completion_query: String::new(),
            completion_trigger_pos: 0,
            chat_area_height: 0,
            // Confirmation dialog
            pending_confirmation: None,
            // Model picker initialization
            show_model_picker: false,
            model_options: {
                let opts = get_model_options_for_provider(&config.llm.provider);
                if config.llm.provider == "custom" {
                    // For custom provider, use the model from config
                    let model = config.llm.model.clone().unwrap_or_else(|| "custom-model".to_string());
                    vec![model]
                } else {
                    opts
                }
            },
            model_selected: 0,
            // Provider picker initialization
            show_provider_picker: false,
            provider_options: vec!["deepseek".to_string(), "openrouter".to_string(), "custom".to_string()],
            provider_selected: {
                let p = config.llm.provider.as_str();
                match p {
                    "openrouter" => 1,
                    "custom" => 2,
                    _ => 0,
                }
            },
            // Session picker initialization
            show_session_picker: false,
            session_options: Vec::new(),
            session_selected: 0,
            init_rx: None,
            confirmation_rx: None,
            shell_mode: false,
            message_queue: Vec::new(),
            show_inline_reasoning: false,
            input_history: Vec::new(),
            history_index: None,
            history_draft: String::new(),
            cursor_anim_start: Instant::now(),
            collapsed_sections: std::collections::HashSet::new(),
            collapsed_toggles: Vec::new(),
            // Code Review state
            orchestrator: None,
            is_reviewing: false,
            review_event_rx: None,
            review_result_rx: None,
            review_iteration: 0,
            review_complete_message: None,
            review_complete_verdict: None,
            review_complete_timer: 0,
            review_reasoning: String::new(),
            previous_review_issues: Vec::new(),
        }
    }
}

/// Return the list of model options for the given provider
pub fn get_model_options_for_provider(provider: &str) -> Vec<String> {
    match provider {
        "deepseek" => vec!["deepseek-chat".to_string(), "deepseek-reasoner".to_string()],
        "openrouter" => vec![
            // ── DeepSeek V4 ──────────────────────────────────────────────
            "deepseek/deepseek-v4-flash".to_string(),
            "deepseek/deepseek-v4-pro".to_string(),
        ],
        "custom" => vec!["custom-model".to_string()],
        _ => vec!["deepseek-chat".to_string(), "deepseek-reasoner".to_string()],
    }
}
