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
        }
    }

    /// Parse and set plan from detected text
    pub fn parse_plan(&mut self, text: &str) {
        self.steps.clear();
        self.step_status.clear();
        self.active = true;
        self.confirmed = false;
        self.current_step = 0;

        // Extract numbered steps from text
        for line in text.lines() {
            let trimmed = line.trim();
            // Match patterns like: 1. Step description
            // or: 1) Step description
            if let Some(stripped) = trimmed.strip_prefix(|c: char| c.is_ascii_digit()) {
                if stripped.starts_with('.') || stripped.starts_with(')') {
                    if let Some(rest) = stripped.strip_prefix(|c: char| c == '.' || c == ')') {
                        let step_text = rest.trim().to_string();
                        if !step_text.is_empty() {
                            self.steps.push(step_text.clone());
                            self.step_status.insert(step_text, PlanStepStatus::Pending);
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
        self.messages.push("✓ Plan confirmed, proceeding...".to_string());
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

    /// Mark current step as completed and move to next
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
            out.push_str(&format!("    {}. {}\n", i + 1, step));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plan_tracker_new() {
        let tracker = PlanTracker::new();
        assert!(!tracker.has_active_plan());
        assert!(!tracker.is_confirmed());
    }

    #[test]
    fn test_parse_simple_plan() {
        let mut tracker = PlanTracker::new();
        tracker.parse_plan("## Task Plan\n1. First step\n2. Second step\n3. Third step");

        assert!(tracker.has_active_plan());
        assert_eq!(tracker.total_steps(), 3);
    }

    #[test]
    fn test_parse_plan_with_parentheses() {
        let mut tracker = PlanTracker::new();
        tracker.parse_plan("## Plan\n1) First step\n2) Second step");

        assert!(tracker.has_active_plan());
        assert_eq!(tracker.total_steps(), 2);
    }

    #[test]
    fn test_step_progression() {
        let mut tracker = PlanTracker::new();
        tracker.parse_plan("1. Step one\n2. Step two\n3. Step three");

        assert_eq!(tracker.current_step_index(), 1);
        tracker.complete_current_step();
        assert_eq!(tracker.current_step_index(), 2);
        tracker.complete_current_step();
        assert_eq!(tracker.current_step_index(), 3);
        tracker.complete_current_step();
        assert!(tracker.is_completed());
    }

    #[test]
    fn test_progress_display() {
        let mut tracker = PlanTracker::new();
        tracker.parse_plan("1. Step one\n2. Step two\n3. Step three");
        tracker.confirm();

        assert_eq!(tracker.progress_display(), "[○○○] 1/3");
        tracker.complete_current_step();
        assert_eq!(tracker.progress_display(), "[●○○] 2/3");
        tracker.complete_current_step();
        assert_eq!(tracker.progress_display(), "[●●○] 3/3");
    }

    #[test]
    fn test_cancel_plan() {
        let mut tracker = PlanTracker::new();
        tracker.parse_plan("1. Step one");
        assert!(tracker.has_active_plan());

        tracker.cancel();
        assert!(!tracker.has_active_plan());
    }

    #[test]
    fn test_needs_confirmation() {
        let mut tracker = PlanTracker::new();
        tracker.parse_plan("1. Step one");
        assert!(tracker.needs_confirmation());

        tracker.confirm();
        assert!(!tracker.needs_confirmation());
    }
}
