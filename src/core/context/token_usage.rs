use crate::core::config::Config;
use crate::core::types::Usage;
use serde::{Deserialize, Serialize};

const DEFAULT_WARN_THRESHOLD_PERCENT: u64 = 75;
const DEFAULT_CRITICAL_THRESHOLD_PERCENT: u64 = 90;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenUsage {
    usage: Usage,
    context_window: u64,
    last_turn_input_tokens: u64,
    warn_threshold: u64,
    critical_threshold: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextWarning {
    Approaching(u64),
    Critical(u64),
}

impl ContextWarning {
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

    pub fn print(&self) {
        for line in self.format() {
            println!("{}", line);
        }
    }

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
            context_window: 131_072,
            last_turn_input_tokens: 0,
            warn_threshold: DEFAULT_WARN_THRESHOLD_PERCENT,
            critical_threshold: DEFAULT_CRITICAL_THRESHOLD_PERCENT,
        }
    }

    pub fn with_config(config: &Config) -> Self {
        Self {
            usage: Usage::default(),
            context_window: config.context.window_size,
            last_turn_input_tokens: 0,
            warn_threshold: config.context.warn_threshold_percent,
            critical_threshold: config.context.critical_threshold_percent,
        }
    }

    pub fn with_context_window(context_window: u64) -> Self {
        Self {
            usage: Usage::default(),
            context_window,
            last_turn_input_tokens: 0,
            warn_threshold: DEFAULT_WARN_THRESHOLD_PERCENT,
            critical_threshold: DEFAULT_CRITICAL_THRESHOLD_PERCENT,
        }
    }

    pub fn add(&mut self, turn_usage: Usage) {
        self.last_turn_input_tokens = turn_usage.input_tokens;
        self.usage += turn_usage;
    }

    pub fn context_window(&self) -> u64 {
        self.context_window
    }

    pub fn input_tokens(&self) -> u64 {
        self.usage.input_tokens
    }

    pub fn output_tokens(&self) -> u64 {
        self.usage.output_tokens
    }

    pub fn total_tokens(&self) -> u64 {
        self.usage.total_tokens
    }

    pub fn context_usage_percent(&self) -> f64 {
        if self.context_window == 0 {
            return 0.0;
        }
        self.last_turn_input_tokens as f64 / self.context_window as f64 * 100.0
    }

    pub fn last_turn_input_tokens(&self) -> u64 {
        self.last_turn_input_tokens
    }

    pub fn cache_hit_rate(&self) -> f64 {
        if self.usage.input_tokens == 0 {
            return 0.0;
        }
        self.usage.cached_input_tokens as f64 / self.usage.input_tokens as f64
    }

    pub fn cache_savings_usd(&self) -> f64 {
        const PRICE_PER_MILLION: f64 = 0.28;
        const CACHE_DISCOUNT: f64 = 0.9;
        self.usage.cached_input_tokens as f64 * PRICE_PER_MILLION / 1_000_000.0 * CACHE_DISCOUNT
    }

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

    pub fn print_session_report(&self) {
        for line in self.format_session_report() {
            println!("{}", line);
        }
    }

    pub fn update_pruned_estimate(&mut self, estimated_tokens: u64) {
        self.last_turn_input_tokens = estimated_tokens;
    }

    pub fn usage(&self) -> &Usage {
        &self.usage
    }

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

pub fn format_turn_usage(turn_usage: &Usage) -> String {
    format!(
        "📊 in: {} · out: {} · total: {}",
        turn_usage.input_tokens, turn_usage.output_tokens, turn_usage.total_tokens,
    )
}

pub fn print_turn_usage(turn_usage: &Usage) {
    println!("  {}", format_turn_usage(turn_usage));
}

pub fn format_context_warning(session_usage: &TokenUsage) -> Vec<String> {
    if let Some(warning) = session_usage.context_warning() {
        warning.format()
    } else {
        Vec::new()
    }
}

pub fn print_context_warning(session_usage: &TokenUsage) {
    if let Some(warning) = session_usage.context_warning() {
        warning.print();
    }
}
