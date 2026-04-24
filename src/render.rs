use colored::*;
use std::io::Write;
use termimad::MadSkin;

// ─────────────────────────────────────────────────────────────────────────────
// MarkdownRenderer — line-buffered Markdown streaming
// ─────────────────────────────────────────────────────────────────────────────

/// Manages line-buffered Markdown rendering during a streaming response.
///
/// Complete lines (ending with `\n`) are rendered through [`termimad`] for rich
/// formatting. The current incomplete line is printed raw for instant visual feedback,
/// then replaced once a newline arrives.
///
/// Known limitation: if the raw `current_line` wraps across multiple physical terminal
/// lines, `\x1b[2K` only erases the cursor's current physical line, leaving orphaned
/// wrapped lines visible briefly until the Markdown render overwrites them. This is
/// rare in typical LLM output where lines are short.
pub struct MarkdownRenderer {
    skin: MadSkin,
    /// Complete lines accumulated since the last flush through termimad.
    complete_lines: String,
    /// Current incomplete line being streamed raw to the terminal.
    current_line: String,
}

impl MarkdownRenderer {
    pub fn new() -> Self {
        Self {
            skin: MadSkin::default(),
            complete_lines: String::new(),
            current_line: String::new(),
        }
    }

    /// Appends incoming text, rendering complete lines through Markdown immediately
    /// and printing partial lines raw for instant feedback.
    pub fn push_text(&mut self, text: &str) {
        if text.contains('\n') {
            // Erase the raw current line before rendering (only if one exists)
            if !self.current_line.is_empty() {
                print!("\r\x1b[2K");
                let _ = std::io::stdout().flush();
            }

            // Split new text at the last newline
            let last_nl = text.rfind('\n').unwrap();
            let before_last_nl = &text[..=last_nl]; // includes the \n

            let after_last_nl = &text[last_nl + 1..];

            // Accumulate current_line + before_last_nl, render through termimad
            self.complete_lines
                .push_str(&std::mem::take(&mut self.current_line));
            self.complete_lines.push_str(before_last_nl);
            self.skin.print_text(&self.complete_lines);
            self.complete_lines.clear();

            // Remaining partial line becomes the new current_line
            self.current_line.push_str(after_last_nl);
            if !self.current_line.is_empty() {
                print!("{}", self.current_line);
                let _ = std::io::stdout().flush();
            }
        } else {
            // No newline — accumulate into current_line, print raw
            self.current_line.push_str(text);
            print!("{}", text);
            let _ = std::io::stdout().flush();
        }
    }

    /// Flushes all buffered text (complete lines + current line) through termimad.
    /// Called when a text segment ends (tool call, final response, stream end).
    pub fn flush(&mut self) {
        if !self.current_line.is_empty() {
            print!("\r\x1b[2K");
            let _ = std::io::stdout().flush();
            self.complete_lines
                .push_str(&std::mem::take(&mut self.current_line));
        }
        if !self.complete_lines.is_empty() {
            self.skin.print_text(&self.complete_lines);
            self.complete_lines.clear();
        }
    }

    /// Returns the current incomplete line content (for testing).
    pub fn current_line(&self) -> &str {
        &self.current_line
    }

    /// Returns the buffered complete lines content (for testing).
    pub fn complete_lines(&self) -> &str {
        &self.complete_lines
    }
}

impl Default for MarkdownRenderer {
    fn default() -> Self {
        Self::new()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// ReasoningTracker — reasoning segment accumulation
// ─────────────────────────────────────────────────────────────────────────────

/// Tracks reasoning (chain-of-thought) segments during a streaming response.
///
/// Reasoning text is buffered per-segment. When a segment ends, a collapsed summary
/// is printed and the text is accumulated into `total_reasoning` for the `think` command.
pub struct ReasoningTracker {
    /// Whether we are currently inside a reasoning segment.
    is_reasoning: bool,
    /// Buffered reasoning text for the current segment (cleared after summary).
    reasoning_buf: String,
    /// Accumulated reasoning across the entire stream (for the `think` command).
    total_reasoning: String,
}

impl ReasoningTracker {
    pub fn new() -> Self {
        Self {
            is_reasoning: false,
            reasoning_buf: String::new(),
            total_reasoning: String::new(),
        }
    }

    /// Returns `true` if currently inside a reasoning segment.
    pub fn is_reasoning(&self) -> bool {
        self.is_reasoning
    }

    /// Starts a new reasoning segment (if not already in one) and appends text.
    pub fn append(&mut self, text: &str) {
        if !self.is_reasoning {
            self.is_reasoning = true;
            print!("\n  {} ", "💭".bright_magenta());
            print!("{}", "Thinking...".bright_magenta().dimmed());
            let _ = std::io::stdout().flush();
        }
        self.reasoning_buf.push_str(text);
    }

    /// Ends the current reasoning segment: prints a collapsed summary,
    /// accumulates the text into `total_reasoning`, and clears the buffer.
    pub fn end_segment(&mut self) {
        self.is_reasoning = false;
        print_reasoning_summary(&self.reasoning_buf);
        if !self.reasoning_buf.is_empty() {
            self.total_reasoning.push_str(&self.reasoning_buf);
            self.total_reasoning.push('\n');
        }
        self.reasoning_buf.clear();
    }

    /// Flushes any in-progress reasoning into `total_reasoning` without printing
    /// a summary. Used when the stream is interrupted or errors out.
    pub fn flush_unfinished(&mut self) {
        if !self.reasoning_buf.is_empty() {
            self.total_reasoning.push_str(&self.reasoning_buf);
            self.total_reasoning.push('\n');
        }
    }

    /// Consumes the tracker and returns the total accumulated reasoning text.
    pub fn into_total_reasoning(self) -> String {
        self.total_reasoning
    }

    /// Returns the current reasoning buffer (for testing).
    pub fn reasoning_buf(&self) -> &str {
        &self.reasoning_buf
    }

    /// Returns the total accumulated reasoning text (for testing).
    pub fn total_reasoning(&self) -> &str {
        &self.total_reasoning
    }
}

impl Default for ReasoningTracker {
    fn default() -> Self {
        Self::new()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Reasoning summary display
// ─────────────────────────────────────────────────────────────────────────────

/// Prints a collapsed summary of the reasoning content.
/// Shows the first line of reasoning (or a truncation hint) so the user knows reasoning occurred
/// without flooding the terminal. The full reasoning can be reviewed with the `think` command.
pub fn print_reasoning_summary(reasoning: &str) {
    if reasoning.is_empty() {
        return;
    }
    // Erase the "Thinking..." line first
    print!("\r\x1b[2K");
    let _ = std::io::stdout().flush();

    // Get first non-empty line as summary
    let first_line = reasoning
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("");

    let char_count = reasoning.len();
    let line_count = reasoning.lines().count();

    // Build display text, handling empty first line
    let display_line = if first_line.is_empty() {
        "(see full reasoning)".to_string()
    } else if first_line.chars().count() > 80 {
        // Truncate first line if too long (char-based to avoid UTF-8 panic)
        let truncated: String = first_line.chars().take(77).collect();
        format!("{}...", truncated)
    } else {
        first_line.to_string()
    };

    println!(
        "  {} {} ({} chars, {} lines) {}",
        "💭".bright_magenta(),
        display_line.bright_magenta().dimmed(),
        char_count.to_string().bright_magenta().dimmed(),
        line_count.to_string().bright_magenta().dimmed(),
        "[type 'think' to expand]".bright_black()
    );
}
