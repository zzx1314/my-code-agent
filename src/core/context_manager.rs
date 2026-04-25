use rig::completion::Message;
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

    pub fn prune_messages(&self, messages: &[Message]) -> Vec<Message> {
        let max_tokens = self.estimate_max_tokens();

        if messages.is_empty() {
            return vec![];
        }

        let mut kept = Vec::with_capacity(messages.len());
        let mut token_count = 0;

        for msg in messages.iter().rev() {
            let msg_tokens = self.estimate_message_tokens(msg);

            if token_count + msg_tokens > max_tokens && !kept.is_empty() {
                break;
            }

            token_count += msg_tokens;
            kept.insert(0, msg.clone());
        }

        if kept.is_empty() && !messages.is_empty() {
            kept.push(messages.last().unwrap().clone());
        }

        kept
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
