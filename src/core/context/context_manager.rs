use crate::core::config::Config;
use crate::core::types::Message;

const PREAMBLE_ESTIMATED_TOKENS: u64 = 1000;
const PRE_SEND_THRESHOLD_PERCENT: u64 = 50;

/// Manages the context window for conversation history, including pruning,
/// compaction, and token estimation.
///
/// Tracks whether pruning has been triggered and how many compactions have
/// occurred to provide visibility into context management operations.
#[derive(Debug, Clone)]
pub struct ContextManager {
    config: Config,
    prune_triggered: bool,
    compact_count: usize,
}

impl ContextManager {
    /// Creates a new `ContextManager` by cloning the provided config.
    pub fn new(config: &Config) -> Self {
        Self {
            config: config.clone(),
            prune_triggered: false,
            compact_count: 0,
        }
    }

    /// Creates a new `ContextManager` that takes ownership of the provided config.
    pub fn with_config(config: Config) -> Self {
        Self {
            config,
            prune_triggered: false,
            compact_count: 0,
        }
    }

    /// Returns `true` if the context should be pruned before sending a request,
    /// based on the estimated token usage reaching the pre-send threshold (50%).
    pub fn should_prune_before_send(&self, estimated_tokens: u64) -> bool {
        let window_size = self.config.context.window_size;
        if window_size == 0 {
            return false;
        }
        let usage_pct = (estimated_tokens * 100).div_ceil(window_size);
        usage_pct >= PRE_SEND_THRESHOLD_PERCENT
    }

    /// Returns `true` if the current token usage has reached the critical
    /// threshold, indicating that compaction should be performed.
    pub fn should_compact(&self, input_tokens: u64) -> bool {
        let threshold = self.config.context.critical_threshold_percent;
        let window_size = self.config.context.window_size;
        if window_size == 0 {
            return false;
        }
        let usage_pct = (input_tokens * 100).div_ceil(window_size);
        usage_pct >= threshold
    }

    /// Returns `true` if token usage is at or above the warn threshold but
    /// below the critical threshold, indicating that the context is approaching
    /// its limit and attention is warranted.
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
    /// Strategy:
    ///   1. Preserve first 4 messages (prompt cache stability).
    ///   2. Keep the last Assistant message intact (model may reference its own tool results).
    ///   3. For other messages: strip tool calls from Assistant messages, drop Tool messages.
    ///   4. Fit remaining messages into budget newest-first.
    pub fn prune_messages(&self, messages: &[Message]) -> Vec<Message> {
        let max_tokens = self.estimate_max_tokens();

        if messages.is_empty() {
            return vec![];
        }

        // Find the last Assistant message (keep intact)
        let last_assistant_idx = messages.iter().rposition(|m| m.role == "assistant");

        let min_prefix = 4.min(messages.len());
        let mut kept: Vec<Message> = Vec::new();
        let mut token_count: u64 = 0;

        for msg in &messages[..min_prefix] {
            token_count += Self::estimate_message_tokens(msg);
            kept.push(msg.clone());
        }

        let middle_start = min_prefix;
        let middle_end = messages.len();

        let mut candidates: Vec<(usize, Message)> = Vec::new();
        for (i, msg) in messages[middle_start..middle_end].iter().enumerate() {
            let actual_idx = middle_start + i;
            let processed = if Some(actual_idx) == last_assistant_idx {
                msg.clone()
            } else {
                Self::strip_tool_content(msg)
            };
            candidates.push((actual_idx, processed));
        }

        // Fit from newest to oldest
        let mut tail_kept: Vec<(usize, Message)> = Vec::new();
        for (idx, msg) in candidates.iter().rev() {
            let msg_tokens = Self::estimate_message_tokens(msg);
            if token_count + msg_tokens <= max_tokens {
                token_count += msg_tokens;
                tail_kept.push((*idx, msg.clone()));
            }
        }
        tail_kept.reverse();

        kept.extend(tail_kept.into_iter().map(|(_, msg)| msg));

        if kept.is_empty() && !messages.is_empty() {
            kept.push(messages.last().unwrap().clone());
        }

        // Ensure tool call chain consistency — DeepSeek rejects requests
        // where an assistant with tool_calls lacks its tool responses.
        Self::ensure_tool_chain_consistency(&mut kept);

        kept
    }

    /// Ensure tool call chain consistency after pruning.
    ///
    /// DeepSeek (OpenAI-compatible) requires that every assistant message
    /// with `tool_calls` is immediately followed by the corresponding tool
    /// result messages. If pruning strips/converts those tool results (e.g.
    /// `strip_tool_content` converts them to user messages), the API
    /// rejects the request with:
    ///   "An assistant message with 'tool_calls' must be followed by tool
    ///    messages responding to each 'tool_call_id'."
    ///
    /// This method handles both directions:
    ///   1. Forward pass: strips `tool_calls` from any assistant whose tool
    ///      responses are missing or altered.
    ///   2. Backward pass: converts orphaned `tool` messages (those whose
    ///      corresponding assistant had `tool_calls` stripped or was removed
    ///      entirely) to `user` messages. Without this, DeepSeek rejects
    ///      tool messages that have no preceding assistant with matching
    ///      `tool_call_id`.
    fn ensure_tool_chain_consistency(messages: &mut Vec<Message>) {
        // Forward pass: strip tool_calls from assistants whose tool results
        // are missing or have been converted.
        let mut i = 0;
        while i < messages.len() {
            if let Some(ref calls) = messages[i].tool_calls.clone() {
                let num_calls = calls.len();
                let chain_ok = (0..num_calls).all(|offset| {
                    let idx = i + 1 + offset;
                    idx < messages.len()
                        && messages[idx].role == "tool"
                        && messages[idx].tool_call_id.as_deref() == Some(&calls[offset].id)
                });
                if !chain_ok {
                    messages[i].tool_calls = None;
                }
            }
            i += 1;
        }

        // Backward pass: collect valid tool_call_ids from assistants that
        // still have tool_calls, then convert orphaned tool messages (whose
        // tool_call_id doesn't match any preceding assistant) to user messages.
        let mut valid_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
        for msg in messages.iter() {
            if let Some(ref calls) = msg.tool_calls {
                for tc in calls {
                    valid_ids.insert(tc.id.clone());
                }
            }
        }
        for msg in messages.iter_mut() {
            if msg.role == "tool" {
                let is_orphan = msg.tool_call_id.as_ref().map_or(true, |id| !valid_ids.contains(id));
                if is_orphan {
                    *msg = Message::user("[tool content removed]");
                }
            }
        }
    }

    /// Strip tool-related content from a message.
    /// For Assistant messages: remove tool_calls.
    /// For Tool messages: replace with placeholder text.
    /// For User/System messages: keep as-is.
    fn strip_tool_content(msg: &Message) -> Message {
        match msg.role.as_str() {
            "assistant" => Message {
                tool_calls: None, // strip tool calls
                ..msg.clone()
            },
            "tool" => Message::user("[tool content removed]"),
            _ => msg.clone(),
        }
    }

    /// Finds the earliest index (from oldest to newest) at which messages
    /// exceed the retention budget (30% of window size) when counted from
    /// the newest end. Used to determine where to insert a summary.
    ///
    /// Returns `None` if all messages fit within the retention budget.
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

    /// Compacts the message list by replacing the oldest messages (those that
    /// exceed the retention budget) with a single summary message.
    ///
    /// Returns a new vector starting with the summary, followed by the messages
    /// that fit within the retention budget.
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

    /// Estimates the maximum number of tokens that can be retained in the
    /// context after applying the warn threshold headroom.
    pub fn estimate_max_tokens(&self) -> u64 {
        let window_size = self.config.context.window_size;
        let warn_threshold = self.config.context.warn_threshold_percent;
        window_size * (100 - warn_threshold) / 100
    }

    /// Estimates the total token count for a list of messages, optionally
    /// including the preamble tokens.
    pub fn estimate_messages_tokens(&self, messages: &[Message], include_preamble: bool) -> u64 {
        let preamble = if include_preamble { PREAMBLE_ESTIMATED_TOKENS } else { 0 };
        let msgs: u64 = messages.iter().map(|m| Self::estimate_message_tokens(m)).sum();
        preamble + msgs
    }

    /// Estimates the number of tokens in a text string using a simple
    /// heuristic: ASCII characters count as 1/4 token, non-ASCII characters
    /// count as 2/3 token. Returns at least 1 for non-empty input.
    pub fn estimate_text_tokens(text: &str) -> u64 {
        if text.is_empty() {
            return 1;
        }
        let ascii = text.chars().filter(|c| c.is_ascii()).count() as u64;
        let non_ascii = text.chars().filter(|c| !c.is_ascii()).count() as u64;
        (ascii / 4 + non_ascii * 2 / 3).max(1)
    }

    /// Estimates the number of tokens in a single message, including content
    /// text, tool call names/arguments, and tool call IDs with fixed overhead.
    pub fn estimate_message_tokens(msg: &Message) -> u64 {
        let mut total = Self::estimate_text_tokens(&msg.content);
        if let Some(ref calls) = msg.tool_calls {
            for call in calls {
                total += Self::estimate_text_tokens(&call.function.name);
                total += Self::estimate_text_tokens(&call.function.arguments);
                total += 5;
            }
        }
        if let Some(ref id) = msg.tool_call_id {
            total += Self::estimate_text_tokens(id);
            total += 5;
        }
        total.max(1)
    }

    /// Returns whether pruning has been triggered during the current session.
    pub fn is_prune_triggered(&self) -> bool {
        self.prune_triggered
    }

    /// Sets the prune-triggered flag to the given value.
    pub fn set_prune_triggered(&mut self, triggered: bool) {
        self.prune_triggered = triggered;
    }

    /// Returns the number of times compaction has been performed during the
    /// current session.
    pub fn compact_count(&self) -> usize {
        self.compact_count
    }

    /// Increments the compaction counter by one.
    pub fn increment_compact_count(&mut self) {
        self.compact_count += 1;
    }

    /// Resets the prune-triggered flag and compaction counter to their
    /// initial values.
    pub fn reset(&mut self) {
        self.prune_triggered = false;
        self.compact_count = 0;
    }
}

/// Formats a slice of messages into a simple human-readable string for
/// display in context logs, showing each message's role and content on
/// separate lines.
pub fn format_messages_for_context(messages: &[Message]) -> String {
    if messages.is_empty() {
        return String::from("(No previous messages)");
    }
    let mut output = String::new();
    for msg in messages {
        let role = &msg.role;
        let content = &msg.content;
        output.push_str(&format!("{}: {}\n", role, content));
    }
    output
}
