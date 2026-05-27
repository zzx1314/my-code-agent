use crate::core::agent::preamble::Agent;
use crate::core::agent::stream_response::{StreamEvent, StreamResult};
use crate::core::config::Config;
use crate::core::context::token_usage::TokenUsage;
use crate::core::skill::SkillManager;
use crate::tools::exec::confirmation::ConfirmationRequest;
use crate::ui::textarea::TextArea;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::Instant;
use tokio::sync::mpsc;

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

    pub fn assistant_with_reasoning(
        content: impl Into<String>,
        reasoning: impl Into<String>,
    ) -> Self {
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
    pub input: TextArea,
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
    /// Completed reasoning segments that appeared AFTER text started (post-text).
    /// Rendered below streaming text but above active reasoning, keeping pre-text
    /// and post-text reasoning sections separate during streaming.
    pub post_text_reasoning: String,
    /// Archived completed pre-text reasoning segments, each representing a distinct
    /// reasoning block that appeared before any text was emitted.
    /// When streaming, each archived segment renders as its own separate thinking
    /// section (with its own "💭 Thinking..." header) instead of being merged into
    /// a single block. Analogous to `completed_post_text_segments` but for the
    /// pre-text thought phase.
    pub completed_pre_text_segments: Vec<String>,
    /// Archived completed post-text reasoning segments, each representing a distinct
    /// reasoning block that appeared after text had already started.
    /// When a new reasoning segment begins while post_text_reasoning already has
    /// content, the old content is moved here so each segment renders separately.
    pub completed_post_text_segments: Vec<String>,
    /// Character boundaries in `streaming_text` marking where each text "chunk"
    /// ends between successive post-text reasoning segments.
    /// Used to interleave text chunks with thinking sections during rendering:
    /// e.g. thought→text[..b0]→archived[0]→text[b0..b1]→post_text→text[b1..]
    pub text_segment_boundaries: Vec<usize>,
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
    /// Extracted first bold header from streaming reasoning (Codex-style)
    pub streaming_reasoning_header: Option<String>,
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
    /// Screen area of the input widget (set each frame during layout).
    pub input_area: ratatui::layout::Rect,
    /// Selection mode toggle: when `true`, mouse tracking is disabled so the
    /// terminal's native click-drag selection works.  When `false` (default),
    /// `?1000h` mouse tracking is active and scroll wheel events are correctly
    /// routed to chat scrolling (not confused with ↑/↓ key events).
    /// Toggled by the user via **Alt+S**.
    pub selection_mode: bool,

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
    pub review_event_rx:
        Option<tokio::sync::mpsc::UnboundedReceiver<crate::app::commands::review::ReviewEvent>>,
    /// Receiver for review result
    pub review_result_rx:
        Option<tokio::sync::mpsc::Receiver<crate::core::types::review::ReviewOutcome>>,
    /// Current auto-review iteration count (0 = first review cycle)
    pub review_iteration: usize,
    /// Transient message to show in status bar after review completes
    pub review_complete_message: Option<String>,
    /// Verdict of the completed review (for color-coding the status bar message)
    pub review_complete_verdict: Option<crate::core::types::review::ReviewVerdict>,
    /// Frames remaining to show review_complete_message (~10 frames/second)
    pub review_complete_timer: u64,
    /// Receiver for /compact async task results
    pub compact_rx: Option<mpsc::Receiver<crate::app::commands::compact::CompactResult>>,
    /// Reasoning content from the review agent's LLM calls.
    /// Displayed on the frontend but NOT added to conversation history.
    pub review_reasoning: String,
    /// Natural language review feedback, streamed incrementally from the review LLM.
    pub review_feedback: String,
    /// Issues from the previous auto-review iteration.
    /// Used for fingerprint-based deduplication to prevent repeated false positives.
    /// Cleared when the review loop ends (approved or max iterations reached).
    pub previous_review_issues: Vec<crate::core::types::review::ReviewIssue>,
    /// Git baseline SHA for incremental review diff detection.
    /// After each review completes, `git stash create` saves the current state.
    /// Next review will `git diff <baseline>` to show only changes since last review.
    pub review_baseline: Option<String>,
    // === Performance caches ===
    /// Cache of rendered markdown lines keyed by content string.
    /// Avoids re-parsing markdown on every frame for unchanged content.
    pub rendered_cache: HashMap<String, Vec<ratatui::text::Line<'static>>>,
    /// Per-frame git diff cache keyed by file path.
    /// Avoids spawning git subprocess multiple times for the same file in one frame.
    pub git_diff_cache: HashMap<String, String>,
    // === Translation state ===
    /// Whether a Chinese→English translation is in progress.
    pub translating: bool,
    /// Receiver for the translation result (oneshot from spawned task).
    pub translation_rx: Option<tokio::sync::oneshot::Receiver<anyhow::Result<String>>>,
    /// Original Chinese text being translated.
    pub translation_original: String,
    /// Computed user message background color (Codex-style: terminal bg + 12% white overlay).
    pub user_message_bg: ratatui::style::Color,
    /// Frosted glass background color for the input area.
    /// Dynamically computed from the terminal's actual background color at startup.
    /// Falls back to `Rgb(48, 48, 52)` when the terminal doesn't support OSC 11.
    pub input_bg_color: ratatui::style::Color,
    /// Y position (0-based) of the chat area in the terminal.
    /// Used by mouse click handling to convert terminal row → content line index.
    pub chat_area_y: u16,
    /// Cooldown deadline: when set, new user messages are delayed until this instant.
    /// Set after a model response completes when `response_interval_ms > 0`.
    pub response_cooldown_until: Option<Instant>,
    /// Skill manager for handling skill activation/deactivation.
    pub skill_manager: SkillManager,
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
        // Codex-style: no block/borders — update_input_style() in ui() sets cursor line style.
        input_area.set_cursor_line_style(ratatui::style::Style::default());
        input_area.set_cursor(0);

        App {
            chat_history,
            current_response: String::new(),
            streaming_tool_result: None,
            streaming_status: String::new(),
            streaming_reasoning_header: None,
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
            post_text_reasoning: String::new(),
            completed_pre_text_segments: Vec::new(),
            completed_post_text_segments: Vec::new(),
            text_segment_boundaries: Vec::new(),
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
                    let model = config
                        .llm
                        .model
                        .clone()
                        .unwrap_or_else(|| "custom-model".to_string());
                    vec![model]
                } else {
                    opts
                }
            },
            model_selected: 0,
            input_area: ratatui::layout::Rect::default(),
            selection_mode: false,
            // Provider picker initialization
            show_provider_picker: false,
            provider_options: vec![
                "deepseek".to_string(),
                "openrouter".to_string(),
                "ollama".to_string(),
                "custom".to_string(),
            ],
            provider_selected: {
                let p = config.llm.provider.as_str();
                match p {
                    "openrouter" => 1,
                    "ollama" => 2,
                    "custom" => 3,
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
            compact_rx: None,
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
            review_feedback: String::new(),
            previous_review_issues: Vec::new(),
            review_baseline: None,
            rendered_cache: HashMap::new(),
            git_diff_cache: HashMap::new(),
            translating: false,
            translation_rx: None,
            translation_original: String::new(),
            user_message_bg: compute_user_message_bg(),
            input_bg_color: ratatui::style::Color::Reset,
            chat_area_y: 0,
            response_cooldown_until: None,
            skill_manager: SkillManager::load(),
        }
    }

    /// Returns `true` if the response cooldown is still active (user must wait
    /// before the next message can be sent to the LLM).
    pub fn is_response_cooldown_active(&self) -> bool {
        self.response_cooldown_until
            .map(|deadline| Instant::now() < deadline)
            .unwrap_or(false)
    }

    /// Returns the remaining cooldown duration, or `None` if the cooldown has
    /// expired or was never set.
    pub fn response_cooldown_remaining(&self) -> Option<std::time::Duration> {
        self.response_cooldown_until.and_then(|deadline| {
            let now = Instant::now();
            if now < deadline {
                Some(deadline - now)
            } else {
                None
            }
        })
    }
}

/// Map a full model name to a short display alias.
///
/// For OpenRouter models (and any others with long names), return a concise
/// alias that fits better in the terminal UI. Unknown/unmapped models are
/// returned as-is.
pub fn model_display_name(model: &str) -> String {
    match model {
        // ── OpenRouter aliases ──
        "deepseek/deepseek-v4-flash" => "DS V4 Flash".to_string(),
        "deepseek/deepseek-v4-pro" => "DS V4 Pro".to_string(),
        "openrouter/owl-alpha" => "Owl Alpha".to_string(),
        // ── DeepSeek aliases ──
        "deepseek-v4-flash" => "DS V4 Flash".to_string(),
        "deepseek-v4-pro" => "DS V4 Pro".to_string(),
        // Fallback: return as-is
        _ => model.to_string(),
    }
}

/// Return the list of model options for the given provider.
///
/// For Ollama, this fetches the locally available models from the Ollama API
/// (`GET /api/tags`). If the request fails (e.g. Ollama is not running),
/// falls back to a small set of common model names.
pub fn get_model_options_for_provider(provider: &str) -> Vec<String> {
    match provider {
        "deepseek" => vec![
            "deepseek-v4-flash".to_string(),
            "deepseek-v4-pro".to_string(),
        ],
        "openrouter" => vec![
            // ── DeepSeek V4 ──────────────────────────────────────────────
            "deepseek/deepseek-v4-flash".to_string(),
            "deepseek/deepseek-v4-pro".to_string(),
            "openrouter/owl-alpha".to_string(),
        ],
        "ollama" => fetch_ollama_models(),
        "custom" => vec!["custom-model".to_string()],
        _ => vec![
            "deepseek-v4-flash".to_string(),
            "deepseek-v4-pro".to_string(),
        ],
    }
}
/// Apply provider configuration (API key env var, model options, model).
/// Shared by the provider picker and `/connect <provider>` command.
pub fn apply_provider_config(app: &mut App, provider_name: &str) {
    app.config.llm.provider = provider_name.to_string();
    if provider_name != "custom" {
        app.config.llm.api_key_env = match provider_name {
            "deepseek" => "DEEPSEEK_API_KEY".to_string(),
            "openrouter" => "OPENROUTER_API_KEY".to_string(),
            "ollama" => "OLLAMA_API_KEY".to_string(),
            _ => String::new(),
        };
        // Clear base_url for non-custom providers — each has a known default
        // URL (e.g. localhost:11434 for Ollama, api.deepseek.com for DeepSeek).
        // Keeping a stale base_url from a previous custom provider would cause
        // requests to go to the wrong endpoint and potentially send wrong auth.
        app.config.llm.base_url = None;
        app.model_options = get_model_options_for_provider(provider_name);
        app.config.llm.model = app.model_options.first().cloned();
        app.model_selected = 0;
    } else {
        // Custom provider: preserve config.toml values (model, api_key_env, base_url)
        let current_model = app.config.llm.model.clone().unwrap_or_default();
        app.model_options = vec![current_model];
        app.model_selected = 0;
    }
}
/** Global singleton for the async HTTP client used to fetch Ollama models.
 *
 * We intentionally use the async `reqwest::Client` (not the blocking variant)
 * to avoid creating any nested tokio runtimes. Since `fetch_ollama_models()`
 * is called from synchronous code that runs inside a tokio runtime, we use
 * `tokio::task::block_in_place` + `Handle::current().block_on()` to bridge
 * the sync→async boundary safely.
 */
static HTTP_CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

/// Fetch the list of locally available models from the Ollama API.
///
/// Queries `GET http://localhost:11434/api/tags` with a short timeout.
/// Returns model names on success, or a fallback list on failure.
/// Compute the user message background color (Codex-style: terminal bg + 12% white overlay).
/// Assumes a dark terminal background (pure black) — the most common case.
/// Produces ~Rgb(32, 32, 36) — very subtle, just enough to visually distinguish user messages.
/// Blend the terminal's background color with a white overlay for a
/// frosted‑glass look.
///
/// Formula: `0.85 × bg + 0.15 × white`, plus a +4 blue cast on B for a
/// cool glass tint.  Falls back to `Color::Reset` (terminal default, i.e.
/// transparent) when `os_11_bg` is `None` — the input area will simply
/// show the terminal's own background.
pub fn compute_frosted_glass_color(os_11_bg: Option<(u8, u8, u8)>) -> ratatui::style::Color {
    let (r, g, b) = match os_11_bg {
        Some(bg) => bg,
        None => return ratatui::style::Color::Reset,
    };

    let fr = (r as f32 * 0.85 + 38.0).round().clamp(0.0, 255.0) as u8;
    let fg = (g as f32 * 0.85 + 38.0).round().clamp(0.0, 255.0) as u8;
    let fb = (b as f32 * 0.85 + 42.0).round().clamp(0.0, 255.0) as u8;

    ratatui::style::Color::Rgb(fr, fg, fb)
}

fn compute_user_message_bg() -> ratatui::style::Color {
    // Very subtle gray for user messages in chat history — barely visible,
    // just enough to visually distinguish user messages from terminal bg.
    // Input area uses terminal default (transparent), so this bg only
    // affects chat messages.
    ratatui::style::Color::Rgb(32, 32, 36)
}

fn fetch_ollama_models() -> Vec<String> {
    let fallback = vec![
        "llama3.2".to_string(),
        "llama3.1".to_string(),
        "codellama".to_string(),
    ];

    let client = HTTP_CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(2))
            .build()
            .expect("Failed to build reqwest Client")
    });

    // Bridge sync→async safely: block_in_place yields the current async task,
    // allowing us to use Handle::current().block_on() without panicking.
    tokio::task::block_in_place(|| {
        let handle = tokio::runtime::Handle::current();

        let resp = match handle.block_on(client.get("http://localhost:11434/api/tags").send()) {
            Ok(r) => r,
            Err(e) => {
                tracing::debug!(error = %e, "Failed to fetch Ollama models, using fallback");
                return fallback;
            }
        };

        if !resp.status().is_success() {
            tracing::debug!(status = %resp.status(), "Ollama /api/tags returned non-success, using fallback");
            return fallback;
        }

        let json: serde_json::Value = match handle.block_on(resp.json()) {
            Ok(j) => j,
            Err(e) => {
                tracing::debug!(error = %e, "Failed to parse Ollama /api/tags response, using fallback");
                return fallback;
            }
        };

        let models = json["models"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|m| m["name"].as_str().map(|s| s.to_string()))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        if models.is_empty() {
            tracing::debug!("Ollama returned empty model list, using fallback");
            fallback
        } else {
            tracing::info!(count = models.len(), "Fetched Ollama models");
            models
        }
    })
}
