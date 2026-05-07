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
            let trimmed = line.trim();
            // Match patterns like: 1. Step description
            // or: 1) Step description
            if let Some(stripped) = trimmed.strip_prefix(|c: char| c.is_ascii_digit()) {
                if stripped.starts_with('.') || stripped.starts_with(')') {
                    if let Some(rest) = stripped.strip_prefix(|c: char| c == '.' || c == ')') {
                        let raw = rest.trim();
                        // Strip trailing completion marker (✓ or checkmark)
                        let step_text = raw
                            .trim_end_matches('✓')
                            .trim_end_matches(|c: char| c.is_whitespace())
                            .to_string();
                        if !step_text.is_empty() {
                            self.steps.push(step_text.clone());
                            self.step_status.insert(step_text, PlanStepStatus::Pending);
                        }
                    }
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
            let trimmed = line.trim();

            // Check if this line is a numbered step (1. or 1))
            if let Some(stripped) = trimmed.strip_prefix(|c: char| c.is_ascii_digit()) {
                if stripped.starts_with('.') || stripped.starts_with(')') {
                    if let Some(rest) = stripped.strip_prefix(|c: char| c == '.' || c == ')') {
                        let step_text_raw = rest.trim();

                        // Detect ✓ marker at end of the step line
                        let has_marker = step_text_raw.ends_with('\u{2713}');
                        // Strip trailing ✓ and whitespace for matching
                        let step_text = step_text_raw
                            .trim_end_matches('\u{2713}')
                            .trim_end_matches(|c: char| c.is_whitespace())
                            .to_string();

                        if !step_text.is_empty() && has_marker {
                            if let Some(idx) = self.steps.iter().position(|s| s == &step_text) {
                                // Only mark as completed if this step hasn't been completed yet
                                // and is at or beyond the current_step (forward-only progression)
                                let status = self.step_status.get(&step_text);
                                let already_completed =
                                    status == Some(&PlanStepStatus::Completed);
                                if !already_completed && idx >= self.current_step {
                                    let step = &self.steps[idx];
                                    self.step_status
                                        .insert(step.clone(), PlanStepStatus::Completed);
                                    // Update current_step to point to next uncompleted step
                                    if idx >= self.current_step {
                                        self.current_step = idx + 1;
                                    }
                                }
                            }
                        }
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

    /// After a tool call, ensure at least one step is marked as completed.
    ///
    /// First tries `update_from_text` (model self-reporting with ✓ markers).
    /// If no new step was completed by markers, auto-completes the current
    /// pending step since the tool call itself indicates progress.
    pub fn update_and_ensure_progress(&mut self, text: &str) {
        if !self.has_active_plan() || !self.is_confirmed() {
            return;
        }
        let step_before = self.current_step;
        self.update_from_text(text);
        // If no progress was made and current step is still pending, auto-complete
        if self.current_step == step_before
            && self.current_step < self.steps.len()
            && self.step_status.get(&self.steps[self.current_step])
                == Some(&PlanStepStatus::Pending)
        {
            let step = self.steps[self.current_step].clone();
            self.step_status
                .insert(step, PlanStepStatus::Completed);
            self.current_step += 1;
        }
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
