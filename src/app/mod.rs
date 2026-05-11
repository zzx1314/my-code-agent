use crate::core::config::Config;
use crate::core::preamble::Agent;
use crate::core::streaming::{StreamEvent, StreamResult};
use crate::core::token_usage::TokenUsage;
use crate::tools::confirmation::ConfirmationRequest;
use std::sync::Arc;
use tokio::sync::mpsc;
use tui_textarea::TextArea;

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
pub struct InitResult {
    pub message: String,
    pub new_agent: Option<Agent>,
}

pub mod conversion;
pub mod event_handler;
pub mod lifecycle;
pub mod ui;

/// Application state
pub struct App {
    pub chat_history: Vec<(String, String)>,
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
    /// Currently executing tool name (displayed inline, replaces previous)
    pub current_tool_call: Option<String>,
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
    /// Completion item list
    pub completion_items: Vec<String>,
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
    // === Input history ===
    /// History of previously sent input texts (newest last)
    pub input_history: Vec<String>,
    /// Current position in input history while navigating; None = not browsing history.
    /// Index 0 is the oldest entry, last index is the newest.
    pub history_index: Option<usize>,
    /// Draft text saved when the user starts browsing history (so we can restore it on Down past the end)
    pub history_draft: String,
}

impl App {
    /// Create a new App instance
    pub fn new(
        chat_history: Vec<(String, String)>,
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
        }
    }
}

/// Return the list of model options for the given provider
pub fn get_model_options_for_provider(provider: &str) -> Vec<String> {
    match provider {
        "deepseek" => vec!["deepseek-chat".to_string(), "deepseek-reasoner".to_string()],
        "openrouter" => vec![
            // Baidu free models
            "baidu/cobuddy:free".to_string(),
            // OpenRouter free models
            "nvidia/nemotron-3-super-120b-a12b:free".to_string(),
            "inclusionai/ring-2.6-1t:free".to_string(),
            // Poolside free models
            "poolside/laguna-m.1:free".to_string(),
            "poolside/laguna-xs.2:free".to_string(),
            // OpenRouter-specific models
            "openrouter/owl-alpha".to_string(),
        ],
        "custom" => vec!["custom-model".to_string()],
        _ => vec!["deepseek-chat".to_string(), "deepseek-reasoner".to_string()],
    }
}
