use my_code_agent::plan_tracker::{PlanConfirmationResult, PlanStepStatus, PlanTracker};

// ============================================================================
// Construction & Default
// ============================================================================

#[test]
fn test_new_tracker_starts_empty() {
    let tracker = PlanTracker::new();
    assert!(!tracker.has_active_plan());
    assert!(!tracker.needs_confirmation());
    assert!(!tracker.is_confirmed());
    assert!(!tracker.is_completed());
    assert_eq!(tracker.total_steps(), 0);
    assert_eq!(tracker.current_step_index(), 1);
    assert!(tracker.messages().is_empty());
    assert!(tracker.progress_display().is_empty());
    assert!(tracker.format_with_confirmation().is_empty());
}

#[test]
fn test_default_trait() {
    let tracker = PlanTracker::default();
    assert!(!tracker.has_active_plan());
    assert_eq!(tracker.total_steps(), 0);
}

#[test]
fn test_clone() {
    let mut tracker = PlanTracker::new();
    tracker.parse_plan("1. Step one\n2. Step two");
    tracker.confirm();

    let cloned = tracker.clone();
    assert_eq!(cloned.total_steps(), 2);
    assert!(cloned.is_confirmed());
    assert!(cloned.has_active_plan());
}

// ============================================================================
// parse_plan — dot format (1. 2. 3.)
// ============================================================================

#[test]
fn test_parse_plan_dot_format() {
    let mut tracker = PlanTracker::new();
    tracker.parse_plan("1. First step\n2. Second step\n3. Third step");

    assert!(tracker.has_active_plan());
    assert_eq!(tracker.total_steps(), 3);
    assert!(!tracker.is_confirmed());
    assert_eq!(tracker.current_step_index(), 1);
}

#[test]
fn test_parse_plan_dot_format_with_header() {
    let mut tracker = PlanTracker::new();
    tracker.parse_plan("## Task Plan\n1. Read the file\n2. Analyze the code\n3. Write the fix");

    assert!(tracker.has_active_plan());
    assert_eq!(tracker.total_steps(), 3);
}

// ============================================================================
// parse_plan — parenthesis format (1) 2) 3))
// ============================================================================

#[test]
fn test_parse_plan_parenthesis_format() {
    let mut tracker = PlanTracker::new();
    tracker.parse_plan("## Plan\n1) First step\n2) Second step");

    assert!(tracker.has_active_plan());
    assert_eq!(tracker.total_steps(), 2);
}

// ============================================================================
// parse_plan — mixed and edge cases
// ============================================================================

#[test]
fn test_parse_plan_empty_text() {
    let mut tracker = PlanTracker::new();
    tracker.parse_plan("");

    // No steps parsed, plan is "active" but has no steps
    assert!(!tracker.has_active_plan());
    assert_eq!(tracker.total_steps(), 0);
}

#[test]
fn test_parse_plan_no_steps() {
    let mut tracker = PlanTracker::new();
    tracker.parse_plan("This is just regular text\nNo numbered steps here");

    assert!(!tracker.has_active_plan());
    assert_eq!(tracker.total_steps(), 0);
}

#[test]
fn test_parse_plan_resets_state() {
    let mut tracker = PlanTracker::new();
    tracker.parse_plan("1. Old step");
    tracker.confirm();
    tracker.complete_current_step();

    // Re-parsing should reset everything
    tracker.parse_plan("1. New step A\n2. New step B");

    assert_eq!(tracker.total_steps(), 2);
    assert!(!tracker.is_confirmed());
    assert_eq!(tracker.current_step_index(), 1);
}

#[test]
fn test_parse_plan_multi_digit_not_supported() {
    // parse_plan only strips a single leading digit, so steps >=10 won't be parsed
    let mut tracker = PlanTracker::new();
    let text = (1..=12)
        .map(|i| format!("{}. Step {}", i, i))
        .collect::<Vec<_>>()
        .join("\n");

    tracker.parse_plan(&text);
    // Steps 1-9 are parsed; 10, 11, 12 are not (single-digit strip limitation)
    assert_eq!(tracker.total_steps(), 9);
}

#[test]
fn test_parse_plan_single_digit_range() {
    let mut tracker = PlanTracker::new();
    let text = (1..=9)
        .map(|i| format!("{}. Step {}", i, i))
        .collect::<Vec<_>>()
        .join("\n");

    tracker.parse_plan(&text);
    assert_eq!(tracker.total_steps(), 9);
}

#[test]
fn test_parse_plan_skips_empty_step_text() {
    let mut tracker = PlanTracker::new();
    // "1. " with nothing after dot — empty step should be skipped
    tracker.parse_plan("1. Valid step\n2. \n3. Another valid step");

    assert_eq!(tracker.total_steps(), 2);
}

#[test]
fn test_parse_plan_ignores_non_numbered_lines() {
    let mut tracker = PlanTracker::new();
    tracker
        .parse_plan("Some intro text\n1. Step one\nMore text between\n2. Step two\nTrailing text");

    assert_eq!(tracker.total_steps(), 2);
}

#[test]
fn test_parse_plan_single_step() {
    let mut tracker = PlanTracker::new();
    tracker.parse_plan("1. Only one step");

    assert!(tracker.has_active_plan());
    assert_eq!(tracker.total_steps(), 1);
    assert!(!tracker.is_completed());
}

#[test]
fn test_parse_plan_with_blank_lines() {
    let mut tracker = PlanTracker::new();
    tracker.parse_plan("1. Step one\n\n\n2. Step two\n\n3. Step three");

    assert_eq!(tracker.total_steps(), 3);
}

// ============================================================================
// confirm / cancel / needs_confirmation
// ============================================================================

#[test]
fn test_confirm() {
    let mut tracker = PlanTracker::new();
    tracker.parse_plan("1. Do something");

    assert!(tracker.needs_confirmation());
    assert!(!tracker.is_confirmed());

    tracker.confirm();

    assert!(tracker.is_confirmed());
    assert!(!tracker.needs_confirmation());
    assert!(
        tracker
            .messages()
            .iter()
            .any(|m| m.contains("Plan confirmed"))
    );
}

#[test]
fn test_cancel() {
    let mut tracker = PlanTracker::new();
    tracker.parse_plan("1. Step one\n2. Step two");
    assert!(tracker.has_active_plan());

    tracker.cancel();

    assert!(!tracker.has_active_plan());
    assert_eq!(tracker.total_steps(), 0);
    assert!(
        tracker
            .messages()
            .iter()
            .any(|m| m.contains("Plan cancelled"))
    );
}

#[test]
fn test_cancel_after_progress() {
    let mut tracker = PlanTracker::new();
    tracker.parse_plan("1. Step one\n2. Step two\n3. Step three");
    tracker.confirm();
    tracker.complete_current_step();

    assert_eq!(tracker.current_step_index(), 2);

    tracker.cancel();

    assert!(!tracker.has_active_plan());
    assert!(!tracker.is_completed());
}

#[test]
fn test_needs_confirmation_no_plan() {
    let tracker = PlanTracker::new();
    assert!(!tracker.needs_confirmation());
}

#[test]
fn test_needs_confirmation_after_cancel() {
    let mut tracker = PlanTracker::new();
    tracker.parse_plan("1. Step one");
    assert!(tracker.needs_confirmation());

    tracker.cancel();
    assert!(!tracker.needs_confirmation());
}

// ============================================================================
// complete_current_step / is_completed / current_step_index
// ============================================================================

#[test]
fn test_step_progression() {
    let mut tracker = PlanTracker::new();
    tracker.parse_plan("1. Step one\n2. Step two\n3. Step three");

    assert_eq!(tracker.current_step_index(), 1);
    assert!(!tracker.is_completed());

    tracker.complete_current_step();
    assert_eq!(tracker.current_step_index(), 2);
    assert!(!tracker.is_completed());

    tracker.complete_current_step();
    assert_eq!(tracker.current_step_index(), 3);
    assert!(!tracker.is_completed());

    tracker.complete_current_step();
    assert!(tracker.is_completed());
}

#[test]
fn test_complete_beyond_range() {
    let mut tracker = PlanTracker::new();
    tracker.parse_plan("1. Only step");

    tracker.complete_current_step();
    assert!(tracker.is_completed());

    // Calling again should not panic
    tracker.complete_current_step();
    assert!(tracker.is_completed());
}

#[test]
fn test_complete_on_empty_plan() {
    let mut tracker = PlanTracker::new();
    // Should not panic
    tracker.complete_current_step();
    assert!(!tracker.is_completed());
}

#[test]
fn test_current_step_index_is_one_based() {
    let mut tracker = PlanTracker::new();
    tracker.parse_plan("1. Step one\n2. Step two");

    // Before any completion, index is 1
    assert_eq!(tracker.current_step_index(), 1);
    tracker.complete_current_step();
    assert_eq!(tracker.current_step_index(), 2);
}

// ============================================================================
// progress_display
// ============================================================================

#[test]
fn test_progress_display_initial() {
    let mut tracker = PlanTracker::new();
    tracker.parse_plan("1. Step one\n2. Step two\n3. Step three");

    let display = tracker.progress_display();
    assert_eq!(display, "[○○○] 1/3");
}

#[test]
fn test_progress_display_after_one_step() {
    let mut tracker = PlanTracker::new();
    tracker.parse_plan("1. Step one\n2. Step two\n3. Step three");

    tracker.complete_current_step();
    assert_eq!(tracker.progress_display(), "[●○○] 2/3");
}

#[test]
fn test_progress_display_after_two_steps() {
    let mut tracker = PlanTracker::new();
    tracker.parse_plan("1. Step one\n2. Step two\n3. Step three");

    tracker.complete_current_step();
    tracker.complete_current_step();
    assert_eq!(tracker.progress_display(), "[●●○] 3/3");
}

#[test]
fn test_progress_display_fully_completed() {
    let mut tracker = PlanTracker::new();
    tracker.parse_plan("1. Step one\n2. Step two\n3. Step three");

    tracker.complete_current_step();
    tracker.complete_current_step();
    tracker.complete_current_step();
    assert_eq!(tracker.progress_display(), "[●●●] 4/3");
}

#[test]
fn test_progress_display_no_plan() {
    let tracker = PlanTracker::new();
    assert!(tracker.progress_display().is_empty());
}

#[test]
fn test_progress_display_single_step() {
    let mut tracker = PlanTracker::new();
    tracker.parse_plan("1. Only step");

    assert_eq!(tracker.progress_display(), "[○] 1/1");
    tracker.complete_current_step();
    assert_eq!(tracker.progress_display(), "[●] 2/1");
}

// ============================================================================
// format_with_confirmation
// ============================================================================

#[test]
fn test_format_with_confirmation() {
    let mut tracker = PlanTracker::new();
    tracker.parse_plan("1. Read file\n2. Fix bug\n3. Run tests");

    let display = tracker.format_with_confirmation();
    assert!(display.contains("📋 Task Plan"));
    assert!(display.contains("1. Read file"));
    assert!(display.contains("2. Fix bug"));
    assert!(display.contains("3. Run tests"));
    assert!(display.contains("Confirm?"));
}

#[test]
fn test_format_with_confirmation_no_plan() {
    let tracker = PlanTracker::new();
    assert!(tracker.format_with_confirmation().is_empty());
}

#[test]
fn test_format_with_confirmation_after_cancel() {
    let mut tracker = PlanTracker::new();
    tracker.parse_plan("1. Step one");
    tracker.cancel();

    assert!(tracker.format_with_confirmation().is_empty());
}

// ============================================================================
// log_progress
// ============================================================================

#[test]
fn test_log_progress() {
    let mut tracker = PlanTracker::new();
    tracker.parse_plan("1. First step\n2. Second step\n3. Third step");
    tracker.confirm();

    tracker.log_progress();
    let msgs = tracker.messages();
    assert!(
        msgs.iter()
            .any(|m| m.contains("First step") && m.contains("1/3"))
    );
}

#[test]
fn test_log_progress_at_each_step() {
    let mut tracker = PlanTracker::new();
    tracker.parse_plan("1. Alpha\n2. Beta\n3. Gamma");
    tracker.confirm();

    // Step 1
    tracker.log_progress();
    tracker.complete_current_step();

    // Step 2
    tracker.log_progress();
    tracker.complete_current_step();

    // Step 3
    tracker.log_progress();
    tracker.complete_current_step();

    let msgs = tracker.messages();
    assert!(
        msgs.iter()
            .any(|m| m.contains("Alpha") && m.contains("1/3"))
    );
    assert!(msgs.iter().any(|m| m.contains("Beta") && m.contains("2/3")));
    assert!(
        msgs.iter()
            .any(|m| m.contains("Gamma") && m.contains("3/3"))
    );
}

#[test]
fn test_log_progress_skipped_without_confirmation() {
    let mut tracker = PlanTracker::new();
    tracker.parse_plan("1. Step one");
    // Not confirmed

    tracker.log_progress();
    assert!(tracker.messages().is_empty());
}

#[test]
fn test_log_progress_skipped_without_plan() {
    let mut tracker = PlanTracker::new();
    tracker.log_progress();
    assert!(tracker.messages().is_empty());
}

// ============================================================================
// log_completion
// ============================================================================

#[test]
fn test_log_completion() {
    let mut tracker = PlanTracker::new();
    tracker.parse_plan("1. Step one\n2. Step two");
    tracker.complete_current_step();
    tracker.complete_current_step();

    assert!(tracker.is_completed());
    tracker.log_completion();

    assert!(
        tracker
            .messages()
            .iter()
            .any(|m| m.contains("Plan completed"))
    );
}

#[test]
fn test_log_completion_not_yet_done() {
    let mut tracker = PlanTracker::new();
    tracker.parse_plan("1. Step one\n2. Step two");
    tracker.complete_current_step();
    // Only 1 of 2 completed

    tracker.log_completion();
    assert!(
        tracker
            .messages()
            .iter()
            .all(|m| !m.contains("Plan completed"))
    );
}

#[test]
fn test_log_completion_no_plan() {
    let mut tracker = PlanTracker::new();
    // No plan at all — should not panic
    tracker.log_completion();
    assert!(tracker.messages().is_empty());
}

// ============================================================================
// take_messages
// ============================================================================

#[test]
fn test_take_messages() {
    let mut tracker = PlanTracker::new();
    tracker.parse_plan("1. Step one");
    tracker.confirm();
    tracker.log_progress();

    assert!(!tracker.messages().is_empty());

    let taken = tracker.take_messages();
    assert!(!taken.is_empty());
    // After take, messages should be empty
    assert!(tracker.messages().is_empty());
}

#[test]
fn test_take_messages_empty() {
    let mut tracker = PlanTracker::new();
    let taken = tracker.take_messages();
    assert!(taken.is_empty());
}

#[test]
fn test_take_messages_accumulates() {
    let mut tracker = PlanTracker::new();
    tracker.parse_plan("1. Step one\n2. Step two");
    tracker.confirm();

    tracker.log_progress();
    tracker.complete_current_step();
    tracker.log_progress();
    tracker.complete_current_step();
    tracker.log_completion();

    let taken = tracker.take_messages();
    assert!(taken.len() >= 3); // at least 2 progress + 1 completion
    assert!(tracker.messages().is_empty());
}

// ============================================================================
// messages (borrow)
// ============================================================================

#[test]
fn test_messages_reflects_state() {
    let mut tracker = PlanTracker::new();
    tracker.parse_plan("1. Step");

    // Confirm adds a message
    tracker.confirm();
    assert!(tracker.messages().iter().any(|m| m.contains("confirmed")));

    // Cancel adds a message
    let mut tracker2 = PlanTracker::new();
    tracker2.parse_plan("1. Step");
    tracker2.cancel();
    assert!(tracker2.messages().iter().any(|m| m.contains("cancelled")));
}

// ============================================================================
// PlanStepStatus enum
// ============================================================================

#[test]
fn test_plan_step_status_variants() {
    let pending = PlanStepStatus::Pending;
    let in_progress = PlanStepStatus::InProgress;
    let completed = PlanStepStatus::Completed;

    assert_eq!(pending, PlanStepStatus::Pending);
    assert_eq!(in_progress, PlanStepStatus::InProgress);
    assert_eq!(completed, PlanStepStatus::Completed);

    assert_ne!(pending, completed);
    assert_ne!(in_progress, completed);
    assert_ne!(pending, in_progress);
}

#[test]
fn test_plan_step_status_clone() {
    let status = PlanStepStatus::InProgress;
    let cloned = status.clone();
    assert_eq!(status, cloned);
}

#[test]
fn test_plan_step_status_debug() {
    let status = PlanStepStatus::Completed;
    let debug_str = format!("{:?}", status);
    assert_eq!(debug_str, "Completed");
}

// ============================================================================
// PlanConfirmationResult enum
// ============================================================================

#[test]
fn test_plan_confirmation_result_variants() {
    let confirmed = PlanConfirmationResult::Confirmed;
    let cancelled = PlanConfirmationResult::Cancelled;
    let ask_details = PlanConfirmationResult::AskDetails;

    assert_eq!(confirmed, PlanConfirmationResult::Confirmed);
    assert_eq!(cancelled, PlanConfirmationResult::Cancelled);
    assert_eq!(ask_details, PlanConfirmationResult::AskDetails);

    assert_ne!(confirmed, cancelled);
    assert_ne!(confirmed, ask_details);
    assert_ne!(cancelled, ask_details);
}

#[test]
fn test_plan_confirmation_result_clone() {
    let result = PlanConfirmationResult::AskDetails;
    let cloned = result.clone();
    assert_eq!(result, cloned);
}

#[test]
fn test_plan_confirmation_result_debug() {
    assert_eq!(
        format!("{:?}", PlanConfirmationResult::Confirmed),
        "Confirmed"
    );
    assert_eq!(
        format!("{:?}", PlanConfirmationResult::Cancelled),
        "Cancelled"
    );
    assert_eq!(
        format!("{:?}", PlanConfirmationResult::AskDetails),
        "AskDetails"
    );
}

// ============================================================================
// Debug trait on PlanTracker
// ============================================================================

#[test]
fn test_plan_tracker_debug() {
    let tracker = PlanTracker::new();
    let debug_str = format!("{:?}", tracker);
    assert!(debug_str.contains("PlanTracker"));
}

// ============================================================================
// Full lifecycle integration test
// ============================================================================

#[test]
fn test_full_lifecycle() {
    let mut tracker = PlanTracker::new();

    // 1. Parse a plan
    tracker.parse_plan("## Task Plan\n1. Analyze code\n2. Write fix\n3. Run tests");
    assert!(tracker.has_active_plan());
    assert!(tracker.needs_confirmation());
    assert_eq!(tracker.total_steps(), 3);

    // 2. Show confirmation format
    let confirm_display = tracker.format_with_confirmation();
    assert!(confirm_display.contains("📋 Task Plan"));
    assert!(confirm_display.contains("Analyze code"));
    assert!(confirm_display.contains("Write fix"));
    assert!(confirm_display.contains("Run tests"));

    // 3. Confirm the plan
    tracker.confirm();
    assert!(!tracker.needs_confirmation());
    assert!(tracker.is_confirmed());

    // 4. Execute step 1
    tracker.log_progress();
    assert_eq!(tracker.progress_display(), "[○○○] 1/3");
    tracker.complete_current_step();
    assert_eq!(tracker.progress_display(), "[●○○] 2/3");

    // 5. Execute step 2
    tracker.log_progress();
    tracker.complete_current_step();
    assert_eq!(tracker.progress_display(), "[●●○] 3/3");

    // 6. Execute step 3
    tracker.log_progress();
    tracker.complete_current_step();
    assert!(tracker.is_completed());
    assert_eq!(tracker.progress_display(), "[●●●] 4/3");

    // 7. Log and collect messages
    tracker.log_completion();
    let messages = tracker.take_messages();
    assert!(messages.iter().any(|m| m.contains("Plan confirmed")));
    assert!(messages.iter().any(|m| m.contains("Analyze code")));
    assert!(messages.iter().any(|m| m.contains("Write fix")));
    assert!(messages.iter().any(|m| m.contains("Run tests")));
    assert!(messages.iter().any(|m| m.contains("Plan completed")));

    // Messages buffer should now be empty
    assert!(tracker.messages().is_empty());
}

#[test]
fn test_full_lifecycle_with_cancel() {
    let mut tracker = PlanTracker::new();

    tracker.parse_plan("1. Step one\n2. Step two\n3. Step three");
    tracker.confirm();
    tracker.complete_current_step();

    // Cancel mid-execution
    tracker.cancel();
    assert!(!tracker.has_active_plan());
    assert!(!tracker.is_completed());

    let messages = tracker.take_messages();
    assert!(messages.iter().any(|m| m.contains("cancelled")));
}

// ============================================================================
// Edge cases
// ============================================================================

#[test]
fn test_reparse_after_completion() {
    let mut tracker = PlanTracker::new();

    // First plan
    tracker.parse_plan("1. A\n2. B");
    tracker.complete_current_step();
    tracker.complete_current_step();
    assert!(tracker.is_completed());

    // Reparse — state should reset
    tracker.parse_plan("1. X\n2. Y\n3. Z");
    assert_eq!(tracker.total_steps(), 3);
    assert!(!tracker.is_confirmed());
    assert!(!tracker.is_completed());
    assert_eq!(tracker.current_step_index(), 1);
}

#[test]
fn test_parse_plan_with_only_number_and_dot() {
    let mut tracker = PlanTracker::new();
    // "1." followed by no text — should be empty and skipped
    tracker.parse_plan("1.\n2. Valid step\n3.");

    assert_eq!(tracker.total_steps(), 1);
}

#[test]
fn test_progress_display_consistency_with_completion() {
    let mut tracker = PlanTracker::new();
    tracker.parse_plan("1. A\n2. B\n3. C\n4. D\n5. E");

    for i in 0..5 {
        let display = tracker.progress_display();
        // Check that filled count matches completed steps
        let filled_count = display.chars().filter(|&c| c == '●').count();
        let empty_count = display.chars().filter(|&c| c == '○').count();
        assert_eq!(filled_count, i);
        assert_eq!(empty_count, 5 - i);

        tracker.complete_current_step();
    }

    assert!(tracker.is_completed());
}
