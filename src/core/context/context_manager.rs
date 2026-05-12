use rig::completion::AssistantContent;
use rig::completion::Message;
use rig::message::UserContent;

use crate::core::config::Config;

/// Preamble overhead estimate: PREAMBLE_TEMPLATE (~150 lines) + knowledge.md
/// Roughly 500-800 tokens depending on knowledge content. We use 1000 for safety.
const PREAMBLE_ESTIMATED_TOKENS: u64 = 1000;

/// Pre-send pruning threshold: if estimated tokens >= this % of window, prune before sending.
/// Set to 50% to leave room for multi-turn tool call results (which rig adds internally).
const PRE_SEND_THRESHOLD_PERCENT: u64 = 50;

#[derive(Debug, Clone)]
pub struct ContextManager {
    config: Config,
    prune_triggered: bool,
    compact_count: usize,
}

impl ContextManager {
    pub fn new(config: &Config) -> Self {
        Self {
            config: config.clone(),
            prune_triggered: false,
            compact_count: 0,
        }
    }

    pub fn with_config(config: Config) -> Self {
        Self {
            config,
            prune_triggered: false,
            compact_count: 0,
        }
    }

    /// Pre-send budget check: if estimated total tokens >= PRE_SEND_THRESHOLD_PERCENT
    /// of window_size, prune before sending to leave room for multi-turn tool results.
    pub fn should_prune_before_send(&self, estimated_tokens: u64) -> bool {
        let window_size = self.config.context.window_size;
        if window_size == 0 {
            return false;
        }
        let usage_pct = (estimated_tokens * 100).div_ceil(window_size);
        usage_pct >= PRE_SEND_THRESHOLD_PERCENT
    }

    pub fn should_compact(&self, input_tokens: u64) -> bool {
        let threshold = self.config.context.critical_threshold_percent;
        let window_size = self.config.context.window_size;
        if window_size == 0 {
            return false;
        }
        let usage_pct = (input_tokens * 100).div_ceil(window_size);
        usage_pct >= threshold
    }

    pub fn should_warn(&self, input_tokens: u64) -> bool {
        let threshold = self.config.context.warn_threshold_percent;
        let window_size = self.config.context.window_size;
        if window_size == 0 {
            return false;
        }
        let usage_pct = (input_tokens * 100).div_ceil(window_size);
        usage_pct >= threshold && usage_pct < self.config.context.critical_threshold_percent
    }

    /// Prune messages when context window is nearly full.
    ///
    /// Strategy (tool-content-aware):
    ///   1. Preserve the first `min_prefix` messages untouched (for prompt cache stability).
    ///   2. Preserve the last turn (final Assistant message) untouched, because the model
    ///      may still need to reference its own most-recent tool results.
    ///   3. Strip tool content (ToolResult, ToolCall) from ALL other messages.
    ///      This typically removes 80-90% of token weight since file reads, shell output,
    ///      and code review are the dominant context consumers.
    ///   4. Fit stripped messages into budget from newest to oldest, dropping oldest first.
    pub fn prune_messages(&self, messages: &[Message]) -> Vec<Message> {
        let max_tokens = self.estimate_max_tokens();

        if messages.is_empty() {
            return vec![];
        }

        // Phase 1: Identify the "last turn" — the final Assistant message,
        // which we keep with full tool content (model may reference its own recent work).
        let last_assistant_idx = messages.iter().rposition(|m| matches!(m, Message::Assistant { .. }));

        // Phase 2: Preserve prefix (first 4 messages) for cache hit rate
        let min_prefix = 4.min(messages.len());
        let mut kept: Vec<Message> = Vec::new();
        let mut token_count: u64 = 0;

        for msg in &messages[..min_prefix] {
            token_count += Self::estimate_message_tokens(msg);
            kept.push(msg.clone());
        }

        // Phase 3: Build the middle+tail pool with tool content stripped.
        // Keep the last Assistant message untouched.
        let middle_start = min_prefix;
        let middle_end = messages.len();

        let mut candidates: Vec<(usize, Message)> = Vec::new();
        for (i, msg) in messages[middle_start..middle_end].iter().enumerate() {
            let actual_idx = middle_start + i;
            let processed = if Some(actual_idx) == last_assistant_idx {
                // Keep last assistant turn intact
                msg.clone()
            } else {
                // Strip tool content from all other messages
                Self::strip_tool_content(msg)
            };
            candidates.push((actual_idx, processed));
        }

        // Phase 4: Fit candidates from newest to oldest into remaining budget
        // (We want recent context to survive — older messages are dropped first)
        let mut tail_kept: Vec<(usize, Message)> = Vec::new();
        for (idx, msg) in candidates.iter().rev() {
            let msg_tokens = Self::estimate_message_tokens(msg);
            if token_count + msg_tokens <= max_tokens {
                token_count += msg_tokens;
                tail_kept.push((*idx, msg.clone()));
            }
        }
        tail_kept.reverse(); // restore chronological order

        kept.extend(tail_kept.into_iter().map(|(_, msg)| msg));

        // Fallback: always keep at least the last message
        if kept.is_empty() && !messages.is_empty() {
            kept.push(messages.last().unwrap().clone());
        }

        kept
    }

    /// Strip tool-related content from a message, keeping only text.
    ///
    /// For User messages: removes ToolResult, Image, Audio, Video, Document content.
    /// For Assistant messages: removes ToolCall content, keeps Text.
    /// For System messages: no change.
    ///
    /// This is useful because tool outputs (file reads, shell output, etc.) are
    /// typically the largest part of context but become stale after their turn.
    fn strip_tool_content(msg: &Message) -> Message {
        match msg {
            Message::User { content } => {
                // Filter to keep only Text content
                let text_items: Vec<UserContent> = content
                    .iter()
                    .filter(|c| matches!(c, UserContent::Text(_)))
                    .cloned()
                    .collect();

                if let Some(first) = text_items.into_iter().next() {
                    let rest: Vec<UserContent> = content
                        .iter()
                        .filter(|c| matches!(c, UserContent::Text(_)))
                        .skip(1)
                        .cloned()
                        .collect();
                    // Rebuild OneOrMany from filtered items
                    match rig::OneOrMany::many(std::iter::once(first).chain(rest)) {
                        Ok(filtered) => Message::User { content: filtered },
                        Err(_) => {
                            // Should never happen since we checked for at least one item
                            Message::user("[tool content removed]")
                        }
                    }
                } else {
                    // No text content at all — replace with a placeholder
                    Message::user("[tool content removed]")
                }
            }
            Message::Assistant { id, content } => {
                // Filter to keep only Text content
                let text_items: Vec<AssistantContent> = content
                    .iter()
                    .filter(|c| matches!(c, AssistantContent::Text(_)))
                    .cloned()
                    .collect();

                if let Some(first) = text_items.first().cloned() {
                    let rest: Vec<AssistantContent> = text_items.into_iter().skip(1).collect();
                    match rig::OneOrMany::many(std::iter::once(first).chain(rest)) {
                        Ok(filtered) => Message::Assistant {
                            id: id.clone(),
                            content: filtered,
                        },
                        Err(_) => Message::assistant("[tool content removed]"),
                    }
                } else {
                    // No text content — placeholder
                    Message::assistant("[tool content removed]")
                }
            }
            Message::System { .. } => msg.clone(),
        }
    }

    pub fn find_compact_point(&self, messages: &[Message]) -> Option<usize> {
        let retain_tokens = self.config.context.window_size * 30 / 100;

        let mut token_count = 0;
        for (i, msg) in messages.iter().enumerate().rev() {
            let msg_tokens = Self::estimate_message_tokens(msg);
            if token_count + msg_tokens > retain_tokens {
                return Some(i + 1);
            }
            token_count += msg_tokens;
        }

        None
    }

    pub fn compact_messages(&self, messages: &[Message], summary: &str) -> Vec<Message> {
        if messages.is_empty() {
            return vec![];
        }

        let compact_point = self.find_compact_point(messages);

        let mut new_messages = vec![Message::user(format!(
            "Previous conversation summary:\n{}",
            summary
        ))];

        if let Some(point) = compact_point {
            new_messages.extend_from_slice(&messages[point..]);
        }

        new_messages
    }

    fn estimate_max_tokens(&self) -> u64 {
        let window_size = self.config.context.window_size;
        let warn_threshold = self.config.context.warn_threshold_percent;

        window_size * (100 - warn_threshold) / 100
    }

    /// Estimate tokens for a slice of messages. Includes preamble overhead.
    pub fn estimate_messages_tokens(&self, messages: &[Message], include_preamble: bool) -> u64 {
        let preamble = if include_preamble {
            PREAMBLE_ESTIMATED_TOKENS
        } else {
            0
        };
        let msgs: u64 = messages.iter().map(|m| Self::estimate_message_tokens(m)).sum();
        preamble + msgs
    }

    /// Estimate text token count.
    /// ASCII: ~4 chars/token. CJK/non-ASCII: ~1.5 chars/token.
    fn estimate_text_tokens(text: &str) -> u64 {
        if text.is_empty() {
            return 1;
        }
        let ascii = text.chars().filter(|c| c.is_ascii()).count() as u64;
        let non_ascii = text.chars().filter(|c| !c.is_ascii()).count() as u64;
        (ascii / 4 + non_ascii * 2 / 3).max(1)
    }

    fn estimate_message_tokens(msg: &Message) -> u64 {
        match msg {
            Message::User { content } => {
                let mut total = 0u64;
                for item in content.iter() {
                    match item {
                        UserContent::Text(t) => {
                            total += Self::estimate_text_tokens(&t.text);
                        }
                        UserContent::ToolResult(r) => {
                            for tr_content in r.content.iter() {
                                if let rig::completion::message::ToolResultContent::Text(t) = tr_content
                                {
                                    total += Self::estimate_text_tokens(&t.text);
                                } else {
                                    total += 5;
                                }
                            }
                            total += 10;
                        }
                        _ => total += 5,
                    }
                }
                total.max(1)
            }
            Message::Assistant { content, .. } => {
                let mut total = 0u64;
                for item in content.iter() {
                    match item {
                        AssistantContent::Text(t) => {
                            total += Self::estimate_text_tokens(&t.text);
                        }
                        AssistantContent::ToolCall(tc) => {
                            total += Self::estimate_text_tokens(&tc.function.name);
                            let args_str = serde_json::to_string(&tc.function.arguments)
                                .unwrap_or_default();
                            total += Self::estimate_text_tokens(&args_str);
                            total += 5;
                        }
                        _ => total += 5,
                    }
                }
                total.max(1)
            }
            Message::System { content } => Self::estimate_text_tokens(content).max(1),
        }
    }

    pub fn is_prune_triggered(&self) -> bool {
        self.prune_triggered
    }

    pub fn set_prune_triggered(&mut self, triggered: bool) {
        self.prune_triggered = triggered;
    }

    pub fn compact_count(&self) -> usize {
        self.compact_count
    }

    pub fn increment_compact_count(&mut self) {
        self.compact_count += 1;
    }

    pub fn reset(&mut self) {
        self.prune_triggered = false;
        self.compact_count = 0;
    }
}

pub fn format_messages_for_context(messages: &[Message]) -> String {
    if messages.is_empty() {
        return String::from("(No previous messages)");
    }

    let mut output = String::new();

    for msg in messages {
        match msg {
            Message::User { content, .. } => {
                output.push_str(&format!("User: {:?}\n", content));
            }
            Message::Assistant { content, .. } => {
                output.push_str(&format!("Assistant: {:?}\n", content));
            }
            Message::System { content, .. } => {
                output.push_str(&format!("System: {:?}\n", content));
            }
        }
    }

    output
}
