use colored::*;
use rig::completion::Usage;

/// Tracks cumulative token usage across an entire session.
/// Wraps [`rig::completion::Usage`] which implements `AddAssign` for easy accumulation.
#[derive(Debug, Clone)]
pub struct TokenUsage {
    usage: Usage,
}

impl TokenUsage {
    pub fn new() -> Self {
        Self {
            usage: Usage::default(),
        }
    }

    /// Accumulate usage from a single turn's response.
    pub fn add(&mut self, turn_usage: Usage) {
        self.usage += turn_usage;
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
