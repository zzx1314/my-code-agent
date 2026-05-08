use std::collections::HashMap;

/// Tracks the progress of task plan execution
#[derive(Debug, Clone)]
pub struct PlanTracker {
    /// The detected plan steps
    steps: Vec<String>,
    /// Mapping of step text to completion status
    step_status: HashMap<String, PlanStepStatus>,
    /// Whether the plan has been confirmed by the user
    confirmed: bool,
    /// Currently executing step index
    current_step: usize,
    /// Whether the plan is active
    active: bool,
    /// Accumulated status messages for the UI to display
    messages: Vec<String>,
    /// Byte length of the plan text at the time `parse_plan` was called.
    /// Used by `update_from_text` to only scan newly-added text for ✓ markers,
    /// ignoring any pre-existing markers in the initial plan output.
    initial_text_len: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PlanStepStatus {
    Pending,
    InProgress,
    Completed,
}

/// Plan confirmation result
#[derive(Debug, Clone, PartialEq)]
pub enum PlanConfirmationResult {
    /// User confirmed the plan, proceed with execution
    Confirmed,
    /// User rejected/cancelled the plan
    Cancelled,
    /// User wants to see more details
    AskDetails,
}

fn strip_step_prefix(s: &str) -> Option<&str> {
    let s = s.trim();
    let num_end = s.find(|c: char| !c.is_ascii_digit())?;
    if num_end == 0 {
        return None;
    }
    let rest = &s[num_end..];
    if rest.starts_with(". ") || rest.starts_with(") ") {
        Some(&rest[2..])
    } else if rest.starts_with('.') || rest.starts_with(')') {
        Some(rest[1..].trim_start())
    } else {
        None
    }
}

impl PlanTracker {
    /// Create a new plan tracker
    pub fn new() -> Self {
        Self {
            steps: Vec::new(),
            step_status: HashMap::new(),
            confirmed: false,
            current_step: 0,
            active: false,
            messages: Vec::new(),
            initial_text_len: 0,
        }
    }

    /// Parse and set plan from detected text
    pub fn parse_plan(&mut self, text: &str) {
        self.steps.clear();
        self.step_status.clear();
        self.active = true;
        self.confirmed = false;
        self.current_step = 0;
        self.initial_text_len = text.len();

        // Extract numbered steps from text
        for line in text.lines() {
            if let Some(rest) = strip_step_prefix(line) {
                let step_text = rest
                    .trim_end_matches('\u{2713}')
                    .trim_end_matches('✓')
                    .trim()
                    .to_string();
                if !step_text.is_empty() {
                    self.steps.push(step_text.clone());
                    self.step_status.insert(step_text, PlanStepStatus::Pending);
                }
            }
        }
    }

    /// Update step statuses from accumulated plan text.
    ///
    /// Only scans **new** text added after `parse_plan` was called (i.e., text
    /// beyond `initial_text_len`). This prevents ✓ markers that the model
    /// included in its initial plan output from being treated as completion
    /// signals — only markers appended *after* the plan was parsed count.
    ///
    /// Also only allows forward transitions: a step that is already Completed
    /// stays Completed, and only steps >= current_step can be marked.
    pub fn update_from_text(&mut self, text: &str) {
        if !self.has_active_plan() {
            return;
        }

        // Only look at newly-added text after the initial plan output
        let new_text = if text.len() > self.initial_text_len {
            &text[self.initial_text_len..]
        } else {
            return; // No new text to scan
        };

        for line in new_text.lines() {
            if let Some(rest) = strip_step_prefix(line) {
                let has_marker = rest.ends_with('\u{2713}') || rest.ends_with('✓');
                let step_text = rest
                    .trim_end_matches('\u{2713}')
                    .trim_end_matches('✓')
                    .trim()
                    .to_string();

                if step_text.is_empty() || !has_marker {
                    continue;
                }

                let idx = self.steps.iter().position(|s| s == &step_text).or_else(|| {
                    self.steps.iter().position(|s| {
                        s.contains(step_text.as_str()) || step_text.contains(s.as_str())
                    })
                });

                if let Some(idx) = idx {
                    let already_completed =
                        self.step_status.get(&self.steps[idx]) == Some(&PlanStepStatus::Completed);
                    if !already_completed && idx >= self.current_step {
                        let step = self.steps[idx].clone();
                        self.step_status.insert(step, PlanStepStatus::Completed);
                        self.current_step = (0..self.steps.len())
                            .find(|&i| {
                                self.step_status.get(&self.steps[i])
                                    != Some(&PlanStepStatus::Completed)
                            })
                            .unwrap_or(self.steps.len());
                    }
                }
            }
        }
    }

    /// Check if there is an active plan
    pub fn has_active_plan(&self) -> bool {
        self.active && !self.steps.is_empty()
    }

    /// Check if the plan needs confirmation
    pub fn needs_confirmation(&self) -> bool {
        self.has_active_plan() && !self.confirmed
    }

    /// Check if the plan is confirmed
    pub fn is_confirmed(&self) -> bool {
        self.confirmed
    }

    /// Mark the plan as confirmed by user
    pub fn confirm(&mut self) {
        self.confirmed = true;
        self.messages
            .push("✓ Plan confirmed, proceeding...".to_string());
    }

    /// Mark the plan as cancelled
    pub fn cancel(&mut self) {
        self.active = false;
        self.steps.clear();
        self.step_status.clear();
        self.messages.push("✗ Plan cancelled.".to_string());
    }

    /// Get total number of steps
    pub fn total_steps(&self) -> usize {
        self.steps.len()
    }

    /// Get current step index (1-based for display)
    pub fn current_step_index(&self) -> usize {
        self.current_step + 1
    }

    /// Update plan progress from agent text.
    ///
    /// Only scans new text for explicit ✓ markers (e.g. "1. Read file ✓") to detect completed steps.
    /// Does NOT auto-mark steps after each tool call — one step may require multiple tool calls,
    /// so only the model's explicit ✓ markers indicate true completion.
    pub fn update_and_ensure_progress(&mut self, text: &str) {
        if !self.has_active_plan() || !self.is_confirmed() {
            return;
        }
        // Only rely on explicit ✓ markers from the model
        self.update_from_text(text);
    }

    /// Mark current step as completed and move to next.
    ///
    /// This is a manual API for testing or external callers.
    /// In normal flow, the model self-reports completion via ✓ markers
    /// and `update_from_text` handles the state transition.
    pub fn complete_current_step(&mut self) {
        if self.current_step < self.steps.len() {
            let step = &self.steps[self.current_step];
            self.step_status
                .insert(step.clone(), PlanStepStatus::Completed);
            self.current_step += 1;
        }
    }

    /// Check if all steps are completed
    pub fn is_completed(&self) -> bool {
        self.current_step >= self.steps.len() && !self.steps.is_empty()
    }

    /// Get progress display string
    pub fn progress_display(&self) -> String {
        if !self.has_active_plan() {
            return String::new();
        }

        let total = self.total_steps();
        let current = self.current_step_index();

        // Progress bar
        let filled = current.saturating_sub(1);
        let empty = total.saturating_sub(filled);

        let bar = format!("{}{}", "●".repeat(filled), "○".repeat(empty));

        format!("[{}] {}/{}", bar, current, total)
    }

    /// Format the plan with confirmation prompt
    pub fn format_with_confirmation(&self) -> String {
        let mut out = String::new();
        if !self.has_active_plan() {
            return out;
        }

        out.push_str("\n  📋 Task Plan\n");
        for (i, step) in self.steps.iter().enumerate() {
            let status = self.step_status.get(step);
            let marker = match status {
                Some(PlanStepStatus::Completed) => " ✓",
                _ => "",
            };
            out.push_str(&format!("    {}. {}{}\n", i + 1, step, marker));
        }
        out.push_str("  ? Confirm? [Enter=proceed, n=cancel]");
        out
    }

    /// Log a progress step (accumulates to messages)
    pub fn log_progress(&mut self) {
        if !self.has_active_plan() || !self.confirmed {
            return;
        }

        if self.current_step < self.steps.len() {
            let step = &self.steps[self.current_step];
            self.messages.push(format!(
                "⚡ {} ({}/{})",
                step,
                self.current_step_index(),
                self.total_steps()
            ));
        }
    }

    /// Log completion message
    pub fn log_completion(&mut self) {
        if !self.has_active_plan() {
            return;
        }

        if self.is_completed() {
            self.messages.push("✓ Plan completed!".to_string());
        }
    }

    /// Take accumulated messages, leaving the vec empty
    pub fn take_messages(&mut self) -> Vec<String> {
        std::mem::take(&mut self.messages)
    }

    /// Get a reference to accumulated messages
    pub fn messages(&self) -> &[String] {
        &self.messages
    }
}

impl Default for PlanTracker {
    fn default() -> Self {
        Self::new()
    }
}
