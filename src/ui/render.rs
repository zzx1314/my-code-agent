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
