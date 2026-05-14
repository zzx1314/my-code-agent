use crate::core::config::Config;
use crate::core::types::Usage;
use serde::{Deserialize, Serialize};

/// Default percentage of context window usage at which a warning should be shown.
const DEFAULT_WARN_THRESHOLD_PERCENT: u64 = 75;
/// Default percentage of context window usage at which a critical warning should be shown.
const DEFAULT_CRITICAL_THRESHOLD_PERCENT: u64 = 90;

/// Tracks token usage across a conversation session, including cumulative billing
/// counts and context window utilization.
///
/// Provides methods to query usage statistics, compute cache savings, generate
/// context-window warnings, and produce formatted session reports.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenUsage {
    /// Cumulative token usage across all turns in the session (input, output, cached, etc.).
    usage: Usage,
    /// Maximum number of tokens the context window can hold (e.g. 128k for DeepSeek).
    context_window: u64,
    /// Input tokens consumed in the most recent turn, used to compute context usage percent.
    last_turn_input_tokens: u64,
    /// Percentage threshold above which a "warning" is triggered (default: 75%).
    warn_threshold: u64,
    /// Percentage threshold above which a "critical" warning is triggered (default: 90%).
    critical_threshold: u64,
}

/// Indicates the severity of context window utilization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextWarning {
    /// Context usage has exceeded the warning threshold but not the critical threshold.
    /// Contains the threshold percentage.
    Approaching(u64),
    /// Context usage has exceeded the critical threshold — history may be truncated.
    /// Contains the threshold percentage.
    Critical(u64),
}

impl ContextWarning {
    /// Returns a list of formatted user-facing message lines for this warning.
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

    /// Prints the warning messages to stdout, one per line.
    pub fn print(&self) {
        for line in self.format() {
            println!("{}", line);
        }
    }

    /// Returns the threshold percentage associated with this warning level.
    pub fn threshold_percent(&self) -> u64 {
        match self {
            ContextWarning::Approaching(pct) => *pct,
            ContextWarning::Critical(pct) => *pct,
        }
    }
}

impl TokenUsage {
    /// Creates a new `TokenUsage` with default values.
    ///
    /// Context window defaults to 131,072 tokens; warning/critical thresholds
    /// default to 75% and 90% respectively.
    pub fn new() -> Self {
        Self {
            usage: Usage::default(),
            context_window: 131_072,
            last_turn_input_tokens: 0,
            warn_threshold: DEFAULT_WARN_THRESHOLD_PERCENT,
            critical_threshold: DEFAULT_CRITICAL_THRESHOLD_PERCENT,
        }
    }

    /// Creates a new `TokenUsage` initialized from the given [`Config`].
    ///
    /// Reads `context_window`, `warn_threshold_percent`, and `critical_threshold_percent`
    /// from the config's context settings.
    pub fn with_config(config: &Config) -> Self {
        Self {
            usage: Usage::default(),
            context_window: config.context.window_size,
            last_turn_input_tokens: 0,
            warn_threshold: config.context.warn_threshold_percent,
            critical_threshold: config.context.critical_threshold_percent,
        }
    }

    /// Creates a new `TokenUsage` with a custom context window size.
    ///
    /// Warning and critical thresholds use their default values (75% / 90%).
    pub fn with_context_window(context_window: u64) -> Self {
        Self {
            usage: Usage::default(),
            context_window,
            last_turn_input_tokens: 0,
            warn_threshold: DEFAULT_WARN_THRESHOLD_PERCENT,
            critical_threshold: DEFAULT_CRITICAL_THRESHOLD_PERCENT,
        }
    }

    /// Records the token usage from a single turn.
    ///
    /// Updates the cumulative usage and records the current turn's input tokens
    /// for context-percentage calculations.
    pub fn add(&mut self, turn_usage: Usage) {
        self.last_turn_input_tokens = turn_usage.input_tokens;
        self.usage += turn_usage;
    }

    /// Returns the configured context window size (in tokens).
    pub fn context_window(&self) -> u64 {
        self.context_window
    }

    /// Returns the cumulative input tokens across the entire session.
    pub fn input_tokens(&self) -> u64 {
        self.usage.input_tokens
    }

    /// Returns the cumulative output tokens across the entire session.
    pub fn output_tokens(&self) -> u64 {
        self.usage.output_tokens
    }

    /// Returns the cumulative total tokens (input + output) across the session.
    pub fn total_tokens(&self) -> u64 {
        self.usage.total_tokens
    }

    /// Computes the percentage of the context window consumed by the last turn.
    ///
    /// Returns 0.0 if the context window size is 0.
    pub fn context_usage_percent(&self) -> f64 {
        if self.context_window == 0 {
            return 0.0;
        }
        self.last_turn_input_tokens as f64 / self.context_window as f64 * 100.0
    }

    /// Returns the input tokens from the most recent turn.
    pub fn last_turn_input_tokens(&self) -> u64 {
        self.last_turn_input_tokens
    }

    /// Computes the cache hit rate as a fraction of total input tokens.
    ///
    /// Returns 0.0 if no input tokens have been recorded.
    pub fn cache_hit_rate(&self) -> f64 {
        if self.usage.input_tokens == 0 {
            return 0.0;
        }
        self.usage.cached_input_tokens as f64 / self.usage.input_tokens as f64
    }

    /// Computes the estimated dollar savings from cache hits.
    ///
    /// Uses a fixed price per million tokens ($0.28) and a cache discount rate of 90%.
    pub fn cache_savings_usd(&self) -> f64 {
        const PRICE_PER_MILLION: f64 = 0.28;
        const CACHE_DISCOUNT: f64 = 0.9;
        self.usage.cached_input_tokens as f64 * PRICE_PER_MILLION / 1_000_000.0 * CACHE_DISCOUNT
    }

    /// Returns a [`ContextWarning`] if the current context usage exceeds the
    /// configured warning or critical thresholds.
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

    /// Formats a full session usage report as a list of display lines.
    ///
    /// Includes cumulative input/output/total tokens, a context-usage progress bar,
    /// and any applicable warnings.
    pub fn format_session_report(&self) -> Vec<String> {
        let mut lines = Vec::new();
        lines.push(String::new());
        lines.push("  ──────── Token Usage (cumulative billing) ────────".to_string());
        lines.push(format!("  → Input tokens:              {}", self.usage.input_tokens));
        lines.push(format!("  ← Output tokens:             {}", self.usage.output_tokens));
        lines.push(format!("  Σ Total tokens:              {}", self.usage.total_tokens));

        let pct = self.context_usage_percent();
        let remaining = self.context_window.saturating_sub(self.last_turn_input_tokens);
        let bar_width = 20;
        let filled = ((pct as usize) * bar_width / 100).min(bar_width);
        let empty = bar_width - filled;
        let bar_fill = "█";
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

        if let Some(warning) = self.context_warning() {
            lines.extend(warning.format());
        }
        lines.push(String::new());
        lines
    }

    /// Prints the full session usage report to stdout.
    pub fn print_session_report(&self) {
        for line in self.format_session_report() {
            println!("{}", line);
        }
    }

    /// Updates the last-turn input token estimate, typically after a context prune operation.
    pub fn update_pruned_estimate(&mut self, estimated_tokens: u64) {
        self.last_turn_input_tokens = estimated_tokens;
    }

    /// Returns a shared reference to the underlying [`Usage`] value.
    pub fn usage(&self) -> &Usage {
        &self.usage
    }

    /// Resets all cumulative usage counters and the last-turn input token tracker to zero.
    ///
    /// Does **not** reset the context window size or the warning/critical thresholds.
    pub fn reset(&mut self) {
        self.usage = Usage::default();
        self.last_turn_input_tokens = 0;
    }
}

impl Default for TokenUsage {
    fn default() -> Self {
        Self::new()
    }
}

/// Formats a single turn's token usage into a human-readable string.
///
/// Example output: `📊 in: 123 · out: 45 · total: 168`
pub fn format_turn_usage(turn_usage: &Usage) -> String {
    format!(
        "📊 in: {} · out: {} · total: {}",
        turn_usage.input_tokens, turn_usage.output_tokens, turn_usage.total_tokens,
    )
}

/// Prints a single turn's token usage to stdout.
pub fn print_turn_usage(turn_usage: &Usage) {
    println!("  {}", format_turn_usage(turn_usage));
}

/// Returns the formatted context-warning lines for the given session usage, if any.
///
/// Returns an empty [`Vec`] if usage is below both thresholds.
pub fn format_context_warning(session_usage: &TokenUsage) -> Vec<String> {
    if let Some(warning) = session_usage.context_warning() {
        warning.format()
    } else {
        Vec::new()
    }
}

/// Prints the context-warning messages for the given session usage, if any.
pub fn print_context_warning(session_usage: &TokenUsage) {
    if let Some(warning) = session_usage.context_warning() {
        warning.print();
    }
}
