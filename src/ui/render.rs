use ratatui::text::Line;

use super::markdown::{render_full_markdown, render_streaming_markdown as md_render_streaming};

pub struct MarkdownRenderer {
    buffer: String,
}

impl MarkdownRenderer {
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
        }
    }

    pub fn push_text(&mut self, text: &str) {
        self.buffer.push_str(text);
    }

    pub fn flush(&mut self) {}

    pub fn get_buffer(&self) -> &str {
        &self.buffer
    }

    pub fn take_buffer(&mut self) -> String {
        std::mem::take(&mut self.buffer)
    }
}

impl Default for MarkdownRenderer {
    fn default() -> Self {
        Self::new()
    }
}

pub struct ReasoningTracker {
    is_reasoning: bool,
    reasoning_buf: String,
    total_reasoning: String,
}

impl ReasoningTracker {
    pub fn new_with_config(_thinking_display: &str) -> Self {
        Self {
            is_reasoning: false,
            reasoning_buf: String::new(),
            total_reasoning: String::new(),
        }
    }

    pub fn new() -> Self {
        Self::new_with_config("collapsed")
    }

    pub fn is_reasoning(&self) -> bool {
        self.is_reasoning
    }

    pub fn append(&mut self, text: &str) {
        if !self.is_reasoning {
            self.is_reasoning = true;
        }
        // Some reasoning models wrap reasoning content in XML-like tags
        // (e.g. <think>...</think>, <answer>...</answer>). Strip all such
        // HTML/XML-style tags to keep the display clean.
        let cleaned = Self::strip_html_tags(text);
        // Some API providers (e.g., OpenAI-compatible / ChatGPT-format endpoints)
        // return the FULL accumulated reasoning content in each SSE chunk rather
        // than incremental deltas. Detect this: if the new text starts with what
        // we already have, replace the buffer instead of appending — otherwise
        // the content grows exponentially.
        if cleaned.starts_with(self.reasoning_buf.as_str()) {
            self.reasoning_buf = cleaned.to_string();
        } else {
            self.reasoning_buf.push_str(&cleaned);
        }
    }

    /// Strip HTML/XML-like tags (`<tag>`, `</tag>`) from reasoning text.
    ///
    /// Reasoning content is internal monologue — any markup tags in it are
    /// model-internal formatting noise, not meaningful output.
    ///
    /// This handles:
    /// - `<think>`, `</think>`, `<answer>`, `</answer>`, etc.
    ///
    /// It does NOT strip bare `<` that isn't part of a tag (e.g. `x < y`).
    fn strip_html_tags(text: &str) -> String {
        strip_html_tags(text)
    }

    pub fn end_segment(&mut self) {
        self.is_reasoning = false;
        if !self.reasoning_buf.is_empty() {
            self.total_reasoning.push_str(&self.reasoning_buf);
            self.total_reasoning.push('\n');
        }
        self.reasoning_buf.clear();
    }

    pub fn flush_unfinished(&mut self) {
        if !self.reasoning_buf.is_empty() {
            self.total_reasoning.push_str(&self.reasoning_buf);
            self.total_reasoning.push('\n');
            self.reasoning_buf.clear();
        }
    }

    pub fn into_total_reasoning(self) -> String {
        self.total_reasoning
    }

    pub fn reasoning_buf(&self) -> &str {
        &self.reasoning_buf
    }

    pub fn total_reasoning(&self) -> &str {
        &self.total_reasoning
    }

    /// Reset the accumulated total reasoning (e.g. after capturing it for a
    /// tool-call assistant message, so the next loop iteration starts fresh).
    pub fn reset_total(&mut self) {
        self.total_reasoning.clear();
    }
}

impl Default for ReasoningTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// Strip HTML/XML-like tags (`<tag>`, `</tag>`) from text.
///
/// Reasoning content is internal monologue — any markup tags in it are
/// model-internal formatting noise, not meaningful output.
///
/// This handles:
/// - `<think>`, `</think>`, `<answer>`, `</answer>`, etc.
///
/// It does NOT strip bare `<` that isn't part of a tag (e.g. `x < y`).
/// Stateful HTML/XML tag stripper that handles cross-chunk tag boundaries.
///
/// When a tag like `<think>` is split across two streaming chunks
/// (e.g. chunk 1: `"text<thi"`, chunk 2: `"nk>more text"`),
/// this stripper remembers that we're inside an unclosed tag from the
/// previous chunk and discards the remaining tag content before continuing
/// with normal processing.
///
/// # Example
///
/// ```ignore
/// let mut stripper = StatefulTagStripper::new();
/// assert_eq!(stripper.process("text<thi"), "text");
/// assert_eq!(stripper.process("nk>more text"), "more text");
/// ```
pub struct StatefulTagStripper {
    /// True if the previous chunk ended inside an unclosed tag
    inside_tag: bool,
}

impl StatefulTagStripper {
    pub fn new() -> Self {
        Self { inside_tag: false }
    }

    /// Process a chunk of text, stripping HTML/XML tags and handling
    /// cross-chunk tag boundaries.
    pub fn process(&mut self, text: &str) -> String {
        if self.inside_tag {
            // Previous chunk ended inside an unclosed tag.
            // Look for the closing '>' to complete the tag.
            if let Some(end) = text.find('>') {
                self.inside_tag = false;
                // Continue processing from after the '>'
                return self.process(&text[end + 1..]);
            } else {
                // Still no closing '>' — this whole chunk is part of the tag
                return String::new();
            }
        }

        let mut result = String::with_capacity(text.len());
        let mut remaining = text;

        loop {
            if let Some(start) = remaining.find('<') {
                let after = &remaining[start..];
                // A tag starts with < followed by /, _, or an ASCII letter
                let is_tag = after.len() > 1
                    && matches!(after.as_bytes()[1], b'/' | b'_' | b'a'..=b'z' | b'A'..=b'Z');
                if is_tag {
                    if let Some(end) = after.find('>') {
                        // Valid tag — strip the entire <...>
                        result.push_str(&remaining[..start]);
                        remaining = &after[end + 1..];
                    } else {
                        // Unclosed tag — keep text before '<' and remember state
                        result.push_str(&remaining[..start]);
                        self.inside_tag = true;
                        break;
                    }
                } else {
                    // Not a tag (e.g. `x < y`) — keep the '<'
                    result.push_str(&remaining[..start + 1]);
                    remaining = &remaining[start + 1..];
                }
            } else {
                result.push_str(remaining);
                break;
            }
        }

        result
    }

    /// Reset the stripper state (for use between independent streams).
    pub fn reset(&mut self) {
        self.inside_tag = false;
    }

    /// Check if we're currently inside an unclosed tag.
    pub fn is_inside_tag(&self) -> bool {
        self.inside_tag
    }
}

impl Default for StatefulTagStripper {
    fn default() -> Self {
        Self::new()
    }
}

/// Strip HTML/XML-like tags (`<tag>`, `</tag>`) from text.
///
/// This is the **stateless** version — each call is independent and does not
/// remember tags that were split across chunks. For streaming scenarios where
/// tags may be split across chunks, use [`StatefulTagStripper`] instead.
///
/// # Examples
///
/// ```
/// use my_code_agent::ui::render::strip_html_tags;
///
/// assert_eq!(strip_html_tags("hello <think>world</think>"), "hello world");
/// assert_eq!(strip_html_tags("x < y"), "x < y");  // not a tag
/// assert_eq!(strip_html_tags("<think"), "");  // unclosed, dropped
/// assert_eq!(strip_html_tags("text"), "text");
/// ```
pub fn strip_html_tags(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut remaining = text;
    loop {
        if let Some(start) = remaining.find('<') {
            let after = &remaining[start..];
            // A tag starts with < followed by /, _, or an ASCII letter
            let is_tag = after.len() > 1
                && matches!(after.as_bytes()[1], b'/' | b'_' | b'a'..=b'z' | b'A'..=b'Z');
            if is_tag {
                if let Some(end) = after.find('>') {
                    // Valid tag — strip the entire <...>
                    result.push_str(&remaining[..start]);
                    remaining = &after[end + 1..];
                } else {
                    // Unclosed tag — keep text before '<' and stop.
                    // Don't output the incomplete tag or anything after it.
                    // This prevents cross-chunk tag fragment leaks:
                    //   Chunk 1: "<think"  → output nothing (tag unclosed)
                    //   Chunk 2: "some text</think>"  → "some text"
                    // Without this, Chunk 1 would leak "<think" into the display.
                    result.push_str(&remaining[..start]);
                    break;
                }
            } else {
                // Not a tag (e.g. `x < y`) — keep the '<'
                result.push_str(&remaining[..start + 1]);
                remaining = &remaining[start + 1..];
            }
        } else {
            result.push_str(remaining);
            break;
        }
    }
    result
}

pub fn get_reasoning_summary(reasoning: &str) -> String {
    if reasoning.is_empty() {
        return String::new();
    }

    let first_line = reasoning
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("");

    let char_count = reasoning.len();
    let line_count = reasoning.lines().count();

    let display_line = if first_line.is_empty() {
        "(see full reasoning)".to_string()
    } else if first_line.chars().count() > 80 {
        let truncated: String = first_line.chars().take(77).collect();
        format!("{}...", truncated)
    } else {
        first_line.to_string()
    };

    format!(
        "💭 {} ({} chars, {} lines) [type 'think' to expand]",
        display_line, char_count, line_count
    )
}

/// Render streaming markdown text. Delegates to the custom markdown renderer
/// which handles unclosed code blocks natively.
pub fn render_streaming_markdown(text: &str, max_width: Option<usize>) -> Vec<Line<'static>> {
    md_render_streaming(text, max_width)
}

/// Render complete (non-streaming) markdown text.
pub fn render_full(text: &str, max_width: Option<usize>) -> Vec<Line<'static>> {
    render_full_markdown(text, max_width)
}
