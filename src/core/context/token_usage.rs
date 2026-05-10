use crate::core::config::Config;
use rig::completion::Usage;
use serde::{Deserialize, Serialize};

/// Default context window size for DeepSeek V3/R1 (128K = 131,072 tokens).
pub const CONTEXT_WINDOW_SIZE: u64 = 131_072;

/// Default warn threshold (75%).
const DEFAULT_WARN_THRESHOLD_PERCENT: u64 = 75;

/// Default critical threshold (90%).
const DEFAULT_CRITICAL_THRESHOLD_PERCENT: u64 = 90;

// ─────────────────────────────────────────────────────────────────────────────
// Type definitions
// ─────────────────────────────────────────────────────────────────────────────

/// Tracks cumulative token usage across an entire session.
/// Wraps [`rig::completion::Usage`] which implements `AddAssign` for easy accumulation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenUsage {
    usage: Usage,
    /// The model's context window size in tokens.
    context_window: u64,
    /// Effective context size from the most recent API turn.
    /// For DeepSeek (OpenAI-compatible), this is simply `input_tokens` which already
    /// represents the full prompt length including any cached portions.
    last_turn_input_tokens: u64,
    /// Warn threshold percentage (from config).
    warn_threshold: u64,
    /// Critical threshold percentage (from config).
    critical_threshold: u64,
}

/// Context window warning level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextWarning {
    /// Session usage is approaching the context window limit (>= warn threshold %).
    Approaching(u64),
    /// Session usage is critical — conversation may be truncated (>= critical threshold %).
    Critical(u64),
}

// ─────────────────────────────────────────────────────────────────────────────
// Implementations
// ─────────────────────────────────────────────────────────────────────────────

impl ContextWarning {
    /// Format warning messages as lines of text.
    pub fn format(&self) -> Vec<String> {
        match self {
            ContextWarning::Approaching(warn_pct) => {
                vec![format!(
                    "  ⚠ Approaching limit (≥{}%): Context window getting full. Consider using 'clear' to reset history.",
                    warn_pct,
                )]
            }
            ContextWarning::Critical(critical_pct) => {
                vec![
                    format!(
                        "  🔴 Critical (≥{}%): Context window almost full — conversation history may be truncated.",
                        critical_pct,
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
            ContextWarning::Approaching(pct) => *pct,
            ContextWarning::Critical(pct) => *pct,
        }
    }
}

impl TokenUsage {
    pub fn new() -> Self {
        Self {
            usage: Usage::default(),
            context_window: CONTEXT_WINDOW_SIZE,
            last_turn_input_tokens: 0,
            warn_threshold: DEFAULT_WARN_THRESHOLD_PERCENT,
            critical_threshold: DEFAULT_CRITICAL_THRESHOLD_PERCENT,
        }
    }

    /// Creates a `TokenUsage` from the given config, using `config.context.window_size`.
    pub fn with_config(config: &Config) -> Self {
        Self {
            usage: Usage::default(),
            context_window: config.context.window_size,
            last_turn_input_tokens: 0,
            warn_threshold: config.context.warn_threshold_percent,
            critical_threshold: config.context.critical_threshold_percent,
        }
    }

    /// Creates a `TokenUsage` with a custom context window size (for testing).
    pub fn with_context_window(context_window: u64) -> Self {
        Self {
            usage: Usage::default(),
            context_window,
            last_turn_input_tokens: 0,
            warn_threshold: DEFAULT_WARN_THRESHOLD_PERCENT,
            critical_threshold: DEFAULT_CRITICAL_THRESHOLD_PERCENT,
        }
    }

    /// Accumulate usage from a single turn's response.
    pub fn add(&mut self, turn_usage: Usage) {
        // For DeepSeek (OpenAI-compatible API), `input_tokens` (= prompt_tokens) already
        // represents the full prompt length including cached tokens — it IS the context size.
        // Note: Anthropic reports `input_tokens` as only the non-cached portion, and would
        // need `input_tokens + cached_input_tokens + cache_creation_input_tokens`. But since
        // we use DeepSeek, just `input_tokens` is correct.
        self.last_turn_input_tokens = turn_usage.input_tokens;
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

    /// Returns the percentage of the context window that has been used.
    ///
    /// Uses the **last turn's** `input_tokens` as the effective context size.
    /// For DeepSeek (OpenAI-compatible), `input_tokens` (= `prompt_tokens`) already
    /// represents the full prompt length including any cached portions, so no
    /// adjustment is needed.
    ///
    /// Returns the context-window usage percentage from the most recent API turn
    /// with two decimal places of precision.
    pub fn context_usage_percent(&self) -> f64 {
        if self.context_window == 0 {
            return 0.0;
        }
        self.last_turn_input_tokens as f64 / self.context_window as f64 * 100.0
    }

    /// Returns the effective context-window consumption from the most recent API turn.
    /// For DeepSeek (OpenAI-compatible), this is simply `input_tokens` which already
    /// represents the full prompt length.
    pub fn last_turn_input_tokens(&self) -> u64 {
        self.last_turn_input_tokens
    }

    /// Return the cache hit rate (0.0 - 1.0)
    /// Calculated based on cached_input_tokens returned by the API
    pub fn cache_hit_rate(&self) -> f64 {
        if self.usage.input_tokens == 0 {
            return 0.0;
        }
        self.usage.cached_input_tokens as f64 / self.usage.input_tokens as f64
    }

    /// Return the cost savings from caching (in USD)
    /// DeepSeek: cache hit $0.028/M vs cache miss $0.28/M = 90% savings
    pub fn cache_savings_usd(&self) -> f64 {
        // DeepSeek pricing: input $0.28/M, cached $0.028/M
        const PRICE_PER_MILLION: f64 = 0.28;
        const CACHE_DISCOUNT: f64 = 0.9; // 90% discount
        self.usage.cached_input_tokens as f64 * PRICE_PER_MILLION / 1_000_000.0 * CACHE_DISCOUNT
    }

    /// Returns the context window warning level, if any.
    pub fn context_warning(&self) -> Option<ContextWarning> {
        let pct = self.context_usage_percent();
        if pct >= self.critical_threshold as f64 {
            Some(ContextWarning::Critical(self.critical_threshold))
        } else if pct >= self.warn_threshold as f64 {
            Some(ContextWarning::Approaching(self.warn_threshold))
        } else {
            None
        }
    }

    /// Format a detailed session usage report as lines of text.
    ///
    /// Displays:
    /// - **Cumulative billing totals** (input / output / total across all turns).
    /// - **Current context-window usage** (last turn's `input_tokens`, which for DeepSeek
    ///   already represents the full prompt length) with a visual bar.
    pub fn format_session_report(&self) -> Vec<String> {
        let mut lines = Vec::new();

        lines.push(String::new());
        lines.push("  ──────── Token Usage (cumulative billing) ────────".to_string());
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

        // Context window usage bar — uses last_turn_input_tokens (full prompt size for DeepSeek)
        let pct = self.context_usage_percent();
        let remaining = self
            .context_window
            .saturating_sub(self.last_turn_input_tokens);
        let bar_width = 20;
        let filled = ((pct as usize) * bar_width / 100).min(bar_width);
        let empty = bar_width - filled;
        let bar_fill = if pct >= self.critical_threshold as f64 {
            "█"
        } else if pct >= self.warn_threshold as f64 {
            "█"
        } else {
            "█"
        };
        lines.push(format!(
            "  ◈ Context: [{}{}] {:.2}% · {}/{} tokens · {} remaining",
            bar_fill.repeat(filled),
            "░".repeat(empty),
            pct,
            self.last_turn_input_tokens,
            self.context_window,
            remaining,
        ));

        lines.push("  ─────────────────────────────────────────────────".to_string());

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

// ─────────────────────────────────────────────────────────────────────────────
// Free functions
// ─────────────────────────────────────────────────────────────────────────────

/// Format a brief one-line usage summary for a single turn.
pub fn format_turn_usage(turn_usage: &Usage) -> String {
    format!(
        "📊 in: {} · out: {} · total: {}",
        turn_usage.input_tokens, turn_usage.output_tokens, turn_usage.total_tokens,
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
