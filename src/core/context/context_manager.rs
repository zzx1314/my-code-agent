use rig::completion::AssistantContent;
use rig::completion::Message;
use rig::message::UserContent;
use std::collections::VecDeque;

use crate::core::config::Config;

#[derive(Debug, Clone)]
pub struct ContextManager {
    config: Config,
    prune_triggered: bool,
    compact_count: usize,
    message_buffer: VecDeque<Message>,
}

impl ContextManager {
    pub fn new(config: &Config) -> Self {
        Self {
            config: config.clone(),
            prune_triggered: false,
            compact_count: 0,
            message_buffer: VecDeque::new(),
        }
    }

    pub fn with_config(config: Config) -> Self {
        Self {
            config,
            prune_triggered: false,
            compact_count: 0,
            message_buffer: VecDeque::new(),
        }
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
            token_count += self.estimate_message_tokens(msg);
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
            let msg_tokens = self.estimate_message_tokens(msg);
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
            let msg_tokens = self.estimate_message_tokens(msg);
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

    fn estimate_message_tokens(&self, msg: &Message) -> u64 {
        let content = match msg {
            Message::User { content, .. } => format!("{:?}", content),
            Message::Assistant { content, .. } => format!("{:?}", content),
            Message::System { content, .. } => format!("{:?}", content),
        };

        (content.len() as u64 / 4).max(1)
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
        self.message_buffer.clear();
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
