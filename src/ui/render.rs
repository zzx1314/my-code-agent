use ratatui::text::Line;
use tui_markdown::from_str;

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
        self.reasoning_buf.push_str(text);
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
}

impl Default for ReasoningTracker {
    fn default() -> Self {
        Self::new()
    }
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

/// Information about an unclosed code fence in streaming text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeFenceInfo {
    /// Byte position where the unclosed opening fence starts.
    pub open_pos: usize,
    /// Language identifier after the opening ``` (e.g. "rust", "python").
    pub lang: Option<String>,
}

/// Find an unclosed code fence (```) in the text.
///
/// Tracks ```open/close``` pairs: if the total count is odd, the last one is unclosed.
/// Returns `Some(CodeFenceInfo)` with the position and language of the unclosed fence.
pub fn find_unclosed_code_fence(text: &str) -> Option<CodeFenceInfo> {
    let mut in_code_block = false;
    let mut open_pos = 0usize;
    let mut open_lang: Option<String> = None;
    let mut pos = 0;

    for line in text.split('\n') {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            if !in_code_block {
                // Opening fence
                in_code_block = true;
                open_pos = pos;
                let lang_part = trimmed[3..].trim();
                open_lang = if lang_part.is_empty() {
                    None
                } else {
                    Some(lang_part.to_string())
                };
            } else {
                // Closing fence
                in_code_block = false;
            }
        }
        pos += line.len() + 1; // +1 for '\n'
    }

    if in_code_block {
        Some(CodeFenceInfo {
            open_pos,
            lang: open_lang,
        })
    } else {
        None
    }
}

/// Convert `Line<'_>` (borrowing from a temporary) to `Line<'static>` (owned data).
/// This is needed when `from_str` borrows from a local `String` that will be dropped.
fn to_static_line(line: Line<'_>) -> Line<'static> {
    use ratatui::text::Span;
    let alignment = line.alignment;
    let spans: Vec<Span<'static>> = line
        .spans
        .into_iter()
        .map(|span| Span::styled(span.content.into_owned(), span.style))
        .collect();
    let mut result = Line::from(spans);
    result.alignment = alignment;
    result
}

/// Render streaming markdown text with proper handling of incomplete code blocks.
///
/// During streaming, a code fence (```) may be opened but not yet closed because
/// the model is still outputting code. This function detects such cases and wraps
/// the pending content in a temporary closing fence so `tui-markdown` can render
/// it with proper code block styling (background color, syntax highlighting, etc.).
///
/// For text without unclosed code fences, it falls through to `tui-markdown` directly.
pub fn render_streaming_markdown(text: &str) -> Vec<Line<'static>> {
    if text.is_empty() {
        return vec![];
    }

    match find_unclosed_code_fence(text) {
        None => {
            // All code fences are closed — render normally, converting to owned lines
            from_str(text).lines.into_iter().map(to_static_line).collect()
        }
        Some(info) => {
            let prefix = &text[..info.open_pos];
            let pending = &text[info.open_pos..];

            let mut result = Vec::new();

            // Render the complete prefix (everything before the unclosed fence)
            if !prefix.trim().is_empty() {
                result.extend(from_str(prefix).lines.into_iter().map(to_static_line));
            }

            // Wrap the pending content with a temporary closing fence so
            // tui-markdown renders it as a proper code block
            let wrapped = format!("{}\n```", pending);
            result.extend(from_str(&wrapped).lines.into_iter().map(to_static_line));

            result
        }
    }
}

