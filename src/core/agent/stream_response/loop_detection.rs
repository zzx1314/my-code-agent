use std::collections::VecDeque;

/// Tracks recent tool calls to detect when the model repeats itself.
pub(super) struct ToolCallHistory {
    /// Recent (name, normalized_args) pairs. Most recent at the back.
    calls: VecDeque<(String, String)>,
    /// Max entries to keep.
    max_len: usize,
}

impl ToolCallHistory {
    pub fn new() -> Self {
        Self {
            calls: VecDeque::with_capacity(6),
            max_len: 6,
        }
    }

    /// Record a tool call.
    pub fn record(&mut self, name: &str, args: &str) {
        let normalized = Self::normalize(args);
        self.calls.push_back((name.to_string(), normalized));
        while self.calls.len() > self.max_len {
            self.calls.pop_front();
        }
    }

    /// Check if this call is identical to the previous one.
    pub fn is_repeat_of_last(&self, name: &str, args: &str) -> bool {
        let normalized = Self::normalize(args);
        self.calls
            .back()
            .map_or(false, |(n, a)| n == name && a == &normalized)
    }

    /// Normalize arguments: sort keys so semantically identical JSON matches.
    fn normalize(args: &str) -> String {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(args) {
            if let serde_json::Value::Object(map) = v {
                let mut keys: Vec<&String> = map.keys().collect();
                keys.sort();
                let sorted: Vec<String> = keys
                    .iter()
                    .map(|k| {
                        let val = &map[*k];
                        format!("\"{}\":{}", k, val)
                    })
                    .collect();
                return format!("{{{}}}", sorted.join(","));
            }
        }
        args.to_string()
    }

    /// Check consecutive identical call count.
    pub fn consecutive_repeat_count(&self, name: &str, args: &str) -> usize {
        let normalized = Self::normalize(args);
        let mut count = 0;
        for (n, a) in self.calls.iter().rev() {
            if n == name && a == &normalized {
                count += 1;
            } else {
                break;
            }
        }
        count
    }

    /// Count consecutive calls to the same tool regardless of args.
    pub fn consecutive_same_tool_count(&self, name: &str) -> usize {
        let mut count = 0;
        for (n, _) in self.calls.iter().rev() {
            if n == name {
                count += 1;
            } else {
                break;
            }
        }
        count
    }

    /// Build a diagnostic message explaining the loop pattern.
    pub fn build_loop_message(&self, name: &str) -> Option<String> {
        let same_tool_count = self.consecutive_same_tool_count(name);
        if same_tool_count >= 2 {
            Some(format!(
                "[LOOP DETECTED] You have called `{}` {} times in a row (with different arguments each time). \
                 The previous calls did not produce the expected result. \
                 Stop and reassess: re-read the file to see its current state, \
                 then try a different approach.",
                name,
                same_tool_count + 1,
            ))
        } else {
            None
        }
    }

    /// Detect alternating pattern of two tools (A, B, A, B, ...).
    pub fn detect_alternating_pattern(&self) -> Option<String> {
        if self.calls.len() < 4 {
            return None;
        }
        let v: Vec<&str> = self.calls.iter().map(|(n, _)| n.as_str()).collect();
        let len = v.len();

        if v[len - 4] == v[len - 2] && v[len - 3] == v[len - 1] && v[len - 4] != v[len - 3] {
            let a = v[len - 4];
            let b = v[len - 3];
            Some(format!(
                "[LOOP DETECTED] You are alternating between `{}` and `{}`. \
                 Neither approach is producing the expected result. \
                 Stop and reassess: re-read the file to see its current state, \
                 then try a completely different strategy.",
                a, b,
            ))
        } else {
            None
        }
    }
}
