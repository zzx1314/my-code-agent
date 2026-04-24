use colored::*;
use rig::completion::Usage;

/// Default context window size for DeepSeek Reasoner (64K = 65,536 tokens).
pub const CONTEXT_WINDOW_SIZE: u64 = 65_536;

/// Warn when session usage exceeds this percentage of the context window.
const WARN_THRESHOLD_PERCENT: u64 = 75;

/// Critical threshold — conversation history may be truncated.
const CRITICAL_THRESHOLD_PERCENT: u64 = 90;

/// Tracks cumulative token usage across an entire session.
/// Wraps [`rig::completion::Usage`] which implements `AddAssign` for easy accumulation.
#[derive(Debug, Clone)]
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

    /// Print a detailed session usage report.
    pub fn print_session_report(&self) {
        println!();
        println!(
            "{}",
            "  ──────── Token Usage ────────".bright_cyan()
        );
        println!(
            "  {} Input tokens:              {}",
            "→".bright_blue(),
            self.usage.input_tokens.to_string().bright_white()
        );
        println!(
            "  {} Output tokens:             {}",
            "←".bright_magenta(),
            self.usage.output_tokens.to_string().bright_white()
        );
        println!(
            "  {} Total tokens:              {}",
            "Σ".bright_yellow(),
            self.usage.total_tokens.to_string().bright_white().bold()
        );

        // Context window usage bar
        let pct = self.context_usage_percent();
        let remaining = self.context_window.saturating_sub(self.usage.input_tokens);
        let bar_width = 20;
        let filled = ((pct as usize) * bar_width / 100).min(bar_width);
        let empty = bar_width - filled;
        let bar_color = if pct >= CRITICAL_THRESHOLD_PERCENT {
            "█".bright_red()
        } else if pct >= WARN_THRESHOLD_PERCENT {
            "█".bright_yellow()
        } else {
            "█".bright_green()
        };
        println!(
            "  {} Context: [{}{}] {}% · {}/{} tokens · {} remaining",
            "◈".bright_cyan(),
            bar_color.to_string().repeat(filled),
            "░".bright_black().to_string().repeat(empty),
            pct.to_string().bright_white().bold(),
            self.usage.input_tokens.to_string().bright_white(),
            self.context_window.to_string().bright_white(),
            remaining.to_string().bright_white(),
        );

        if self.usage.cached_input_tokens > 0 {
            println!(
                "  {} Cached input tokens:       {}",
                "⛃".bright_green(),
                self.usage.cached_input_tokens.to_string().bright_green()
            );
        }
        if self.usage.cache_creation_input_tokens > 0 {
            println!(
                "  {} Cache creation tokens:     {}",
                "⚙".bright_green(),
                self.usage.cache_creation_input_tokens.to_string().bright_green()
            );
        }
        println!(
            "{}",
            "  ────────────────────────────".bright_cyan()
        );

        // Print warning if approaching or exceeding context limit
        if let Some(warning) = self.context_warning() {
            warning.print();
        }

        println!();
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
    /// Prints a warning message to the terminal.
    pub fn print(&self) {
        match self {
            ContextWarning::Approaching => {
                println!(
                    "  {} {} Context window getting full. Consider using 'clear' to reset history.",
                    "⚠".bright_yellow(),
                    "Approaching limit:".bright_yellow().bold(),
                );
            }
            ContextWarning::Critical => {
                println!(
                    "  {} {} Context window almost full — conversation history may be truncated.",
                    "🔴".bright_red(),
                    "Critical:".bright_red().bold(),
                );
                println!(
                    "  {} Use 'clear' to reset conversation history, or start a new session.",
                    "→".bright_red(),
                );
            }
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

/// Print a brief one-line usage summary for a single turn.
pub fn print_turn_usage(turn_usage: &Usage) {
    println!(
        "  {} in: {} · out: {} · total: {}",
        "📊".dimmed(),
        turn_usage.input_tokens.to_string().bright_cyan(),
        turn_usage.output_tokens.to_string().bright_magenta(),
        turn_usage.total_tokens.to_string().bright_white().bold(),
    );
}

/// Print a context window warning after a turn, if usage is approaching the limit.
pub fn print_context_warning(session_usage: &TokenUsage) {
    if let Some(warning) = session_usage.context_warning() {
        warning.print();
    }
}
