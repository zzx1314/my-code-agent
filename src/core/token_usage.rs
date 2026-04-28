use crate::core::config::Config;
use rig::completion::Usage;
use serde::{Deserialize, Serialize};

/// Default context window size for DeepSeek V3/R1 (128K = 131,072 tokens).
pub const CONTEXT_WINDOW_SIZE: u64 = 131_072;

/// Warn when session usage exceeds this percentage of the context window.
const WARN_THRESHOLD_PERCENT: u64 = 75;

/// Critical threshold — conversation history may be truncated.
const CRITICAL_THRESHOLD_PERCENT: u64 = 90;

/// Tracks cumulative token usage across an entire session.
/// Wraps [`rig::completion::Usage`] which implements `AddAssign` for easy accumulation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenUsage {
    usage: Usage,
    /// The model's context window size in tokens.
    context_window: u64,
}

impl TokenUsage {
    pub fn new() -> Self {
        Self {
            usage: Usage::default(),
            context_window: CONTEXT_WINDOW_SIZE,
        }
    }

    /// Creates a `TokenUsage` from the given config, using `config.context.window_size`.
    pub fn with_config(config: &Config) -> Self {
        Self {
            usage: Usage::default(),
            context_window: config.context.window_size,
        }
    }

    /// Creates a `TokenUsage` with a custom context window size (for testing).
    pub fn with_context_window(context_window: u64) -> Self {
        Self {
            usage: Usage::default(),
            context_window,
        }
    }

    /// Accumulate usage from a single turn's response.
    pub fn add(&mut self, turn_usage: Usage) {
        self.usage += turn_usage;
    }

    /// Returns the context window size in tokens.
    pub fn context_window(&self) -> u64 {
        self.context_window
    }

    /// Returns the total input tokens consumed this session.
    pub fn input_tokens(&self) -> u64 {
        self.usage.input_tokens
    }

    /// Returns the total output tokens consumed this session.
    pub fn output_tokens(&self) -> u64 {
        self.usage.output_tokens
    }

    /// Returns the total tokens consumed this session.
    pub fn total_tokens(&self) -> u64 {
        self.usage.total_tokens
    }

    /// Returns the percentage of the context window that has been used
    /// based on input tokens consumed (output tokens don't consume context window space).
    /// Uses ceiling division so that e.g. 49,151/65,536 ≈ 75.0% rounds up to 75%
    /// rather than truncating to 74%, ensuring warnings fire at the correct boundary.
    pub fn context_usage_percent(&self) -> u64 {
        if self.context_window == 0 {
            return 0;
        }
        (self.usage.input_tokens * 100).div_ceil(self.context_window)
    }

    /// Returns the context window warning level, if any.
    pub fn context_warning(&self) -> Option<ContextWarning> {
        let pct = self.context_usage_percent();
        if pct >= CRITICAL_THRESHOLD_PERCENT {
            Some(ContextWarning::Critical)
        } else if pct >= WARN_THRESHOLD_PERCENT {
            Some(ContextWarning::Approaching)
        } else {
            None
        }
    }

    /// Format a detailed session usage report as lines of text.
    pub fn format_session_report(&self) -> Vec<String> {
        let mut lines = Vec::new();

        lines.push(String::new());
        lines.push("  ──────── Token Usage ────────".to_string());
        lines.push(format!(
            "  → Input tokens:              {}",
            self.usage.input_tokens
        ));
        lines.push(format!(
            "  ← Output tokens:             {}",
            self.usage.output_tokens
        ));
        lines.push(format!(
            "  Σ Total tokens:              {}",
            self.usage.total_tokens
        ));

        // Context window usage bar
        let pct = self.context_usage_percent();
        let remaining = self.context_window.saturating_sub(self.usage.input_tokens);
        let bar_width = 20;
        let filled = ((pct as usize) * bar_width / 100).min(bar_width);
        let empty = bar_width - filled;
        let bar_fill = if pct >= CRITICAL_THRESHOLD_PERCENT {
            "█"
        } else if pct >= WARN_THRESHOLD_PERCENT {
            "█"
        } else {
            "█"
        };
        lines.push(format!(
            "  ◈ Context: [{}{}] {}% · {}/{} tokens · {} remaining",
            bar_fill.repeat(filled),
            "░".repeat(empty),
            pct,
            self.usage.input_tokens,
            self.context_window,
            remaining,
        ));

        if self.usage.cached_input_tokens > 0 {
            lines.push(format!(
                "  ⛃ Cached input tokens:       {}",
                self.usage.cached_input_tokens
            ));
        }
        if self.usage.cache_creation_input_tokens > 0 {
            lines.push(format!(
                "  ⚙ Cache creation tokens:     {}",
                self.usage.cache_creation_input_tokens
            ));
        }
        lines.push("  ────────────────────────────".to_string());

        // Append warning if approaching or exceeding context limit
        if let Some(warning) = self.context_warning() {
            lines.extend(warning.format());
        }

        lines.push(String::new());
        lines
    }

    /// Print a detailed session usage report (for tests/non-TUI usage).
    pub fn print_session_report(&self) {
        for line in self.format_session_report() {
            println!("{}", line);
        }
    }

    /// Returns a reference to the underlying [`Usage`] for programmatic access.
    pub fn usage(&self) -> &Usage {
        &self.usage
    }
}

impl Default for TokenUsage {
    fn default() -> Self {
        Self::new()
    }
}

/// Context window warning level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextWarning {
    /// Session usage is approaching the context window limit (>= 75%).
    Approaching,
    /// Session usage is critical — conversation may be truncated (>= 90%).
    Critical,
}

impl ContextWarning {
    /// Format warning messages as lines of text.
    pub fn format(&self) -> Vec<String> {
        match self {
            ContextWarning::Approaching => {
                vec![
                    format!(
                        "  ⚠ Approaching limit: Context window getting full. Consider using 'clear' to reset history.",
                    ),
                ]
            }
            ContextWarning::Critical => {
                vec![
                    format!(
                        "  🔴 Critical: Context window almost full — conversation history may be truncated.",
                    ),
                    format!(
                        "  → Use 'clear' to reset conversation history, or start a new session.",
                    ),
                ]
            }
        }
    }

    /// Print warnings (for tests/non-TUI usage).
    pub fn print(&self) {
        for line in self.format() {
            println!("{}", line);
        }
    }

    /// Returns the threshold percentage for this warning level.
    pub fn threshold_percent(&self) -> u64 {
        match self {
            ContextWarning::Approaching => WARN_THRESHOLD_PERCENT,
            ContextWarning::Critical => CRITICAL_THRESHOLD_PERCENT,
        }
    }
}

/// Format a brief one-line usage summary for a single turn.
pub fn format_turn_usage(turn_usage: &Usage) -> String {
    format!(
        "📊 in: {} · out: {} · total: {}",
        turn_usage.input_tokens,
        turn_usage.output_tokens,
        turn_usage.total_tokens,
    )
}

/// Print a brief one-line usage summary for a single turn (for tests/non-TUI usage).
pub fn print_turn_usage(turn_usage: &Usage) {
    println!("  {}", format_turn_usage(turn_usage));
}

/// Format context window warnings, if any.
pub fn format_context_warning(session_usage: &TokenUsage) -> Vec<String> {
    if let Some(warning) = session_usage.context_warning() {
        warning.format()
    } else {
        Vec::new()
    }
}

/// Print a context window warning after a turn (for tests/non-TUI usage).
pub fn print_context_warning(session_usage: &TokenUsage) {
    if let Some(warning) = session_usage.context_warning() {
        warning.print();
    }
}
