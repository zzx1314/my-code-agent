use my_code_agent::plan_tracker::PlanTracker;
use my_code_agent::streaming::detect_task_plan;

// ============================================================================
// Bug 1: format_with_confirmation should produce multi-line output
//
// The UI render_chat_area previously treated the entire plan display as a
// single ratatui Line (via Line::from(msg.as_str())), which doesn't interpret
// \n. The fix splits by lines(). These tests verify the data is correct.
// ============================================================================

#[test]
fn test_format_with_confirmation_contains_newlines() {
    let mut tracker = PlanTracker::new();
    tracker.parse_plan("## Task Plan\n1. Read the config\n2. Fix the bug\n3. Run tests");

    let display = tracker.format_with_confirmation();

    // The display must contain newline characters so that the UI can split it
    assert!(
        display.contains('\n'),
        "format_with_confirmation output should contain newline characters for multi-line display"
    );
}

#[test]
fn test_format_with_confirmation_line_count_matches_steps() {
    let mut tracker = PlanTracker::new();
    tracker.parse_plan("1. Step A\n2. Step B\n3. Step C");

    let display = tracker.format_with_confirmation();
    let lines: Vec<&str> = display.lines().collect();

    // Expected structure:
    //   line 0: "\n  📋 Task Plan"  (the leading \n produces an empty first line)
    //   line 1: "    1. Step A"
    //   line 2: "    2. Step B"
    //   line 3: "    3. Step C"
    //   line 4: "  ? Confirm? [Enter=proceed, n=cancel]"
    //
    // format_with_confirmation starts with "\n  📋 Task Plan\n", so .lines()
    // will split into: ["", "  📋 Task Plan", "    1. Step A", "    2. Step B",
    //                    "    3. Step C", "  ? Confirm? [Enter=proceed, n=cancel]"]
    // That's 6 items for 3 steps.
    //
    // Regardless of the exact leading empty line, the important invariant is:
    // the number of step lines (containing "1.", "2.", "3.") equals total_steps().
    let step_lines: Vec<&str> = lines
        .iter()
        .filter(|l| {
            l.trim_start().starts_with("1.")
                || l.trim_start().starts_with("2.")
                || l.trim_start().starts_with("3.")
        })
        .copied()
        .collect();

    assert_eq!(
        step_lines.len(),
        3,
        "Expected 3 step lines, got {}. Lines: {:?}",
        step_lines.len(),
        lines
    );
}

#[test]
fn test_format_with_confirmation_single_step_has_newline() {
    let mut tracker = PlanTracker::new();
    tracker.parse_plan("1. Only step");

    let display = tracker.format_with_confirmation();
    let lines: Vec<&str> = display.lines().collect();

    // At minimum: header line, step line, confirm line
    assert!(
        lines.len() >= 3,
        "Single step plan should have at least 3 lines, got {}",
        lines.len()
    );
}

// ============================================================================
// Bug 2: format_with_confirmation should reflect step completion status
//
// Previously, the plan display was only rendered once (all steps Pending).
// After update_from_text() updates internal state, format_with_confirmation()
// should return updated text with ✓ markers.
// ============================================================================

#[test]
fn test_format_with_confirmation_shows_checkmark_after_complete_current_step() {
    let mut tracker = PlanTracker::new();
    tracker.parse_plan("1. Read file\n2. Fix bug\n3. Run tests");

    // Before any completion — no ✓
    let initial = tracker.format_with_confirmation();
    assert!(
        !initial.contains("✓"),
        "Initial display should have no checkmarks, got: {}",
        initial
    );

    // Complete step 1
    tracker.complete_current_step();
    let after_step1 = tracker.format_with_confirmation();
    let check_count = after_step1.matches("✓").count();
    assert_eq!(
        check_count, 1,
        "After completing step 1, should have 1 checkmark. Display:\n{}",
        after_step1
    );
    assert!(
        after_step1.contains("Read file ✓"),
        "First step should have checkmark. Display:\n{}",
        after_step1
    );

    // Complete step 2
    tracker.complete_current_step();
    let after_step2 = tracker.format_with_confirmation();
    let check_count = after_step2.matches("✓").count();
    assert_eq!(
        check_count, 2,
        "After completing step 2, should have 2 checkmarks. Display:\n{}",
        after_step2
    );

    // Complete step 3
    tracker.complete_current_step();
    let after_step3 = tracker.format_with_confirmation();
    let check_count = after_step3.matches("✓").count();
    assert_eq!(
        check_count, 3,
        "After completing all steps, should have 3 checkmarks. Display:\n{}",
        after_step3
    );
}

#[test]
fn test_format_with_confirmation_shows_checkmark_after_update_from_text() {
    // This tests the real streaming scenario: model self-reports via ✓ markers
    let mut tracker = PlanTracker::new();
    let plan_text = "1. Read file\n2. Fix bug\n3. Run tests";
    tracker.parse_plan(plan_text);
    tracker.confirm();

    // Before any update — no ✓
    let initial = tracker.format_with_confirmation();
    assert!(!initial.contains("✓"));

    // Simulate model appending new text with step 1 ✓
    let accumulated = format!("{}\n1. Read file ✓", plan_text);
    tracker.update_from_text(&accumulated);

    let after_update = tracker.format_with_confirmation();
    let check_count = after_update.matches("✓").count();
    assert_eq!(
        check_count, 1,
        "After update_from_text marks step 1, display should show 1 checkmark. Display:\n{}",
        after_update
    );
    assert!(
        after_update.contains("Read file ✓"),
        "Step 1 should have checkmark in display. Display:\n{}",
        after_update
    );
    assert!(
        !after_update.contains("Fix bug ✓"),
        "Step 2 should NOT have checkmark yet. Display:\n{}",
        after_update
    );
}

#[test]
fn test_format_with_confirmation_all_completed_via_update_from_text() {
    let mut tracker = PlanTracker::new();
    let plan_text = "1. Step A\n2. Step B\n3. Step C";
    tracker.parse_plan(plan_text);
    tracker.confirm();

    // Simulate all steps completed via new text
    let accumulated = format!("{}\n1. Step A ✓\n2. Step B ✓\n3. Step C ✓", plan_text);
    tracker.update_from_text(&accumulated);

    let display = tracker.format_with_confirmation();
    let check_count = display.matches("✓").count();
    assert!(
        check_count >= 3,
        "All 3 steps should have checkmarks. Display:\n{}",
        display
    );
}

// ============================================================================
// Streaming flow simulation: status_messages refresh
//
// This simulates the exact flow from stream_inner in streaming.rs:
// 1. Detect plan → parse_plan → format_with_confirmation → push to status_messages
// 2. After tool call: update_from_text → format_with_confirmation → refresh status_messages
// 3. Verify the refreshed status_messages contain ✓
// ============================================================================

#[test]
fn test_streaming_flow_status_messages_refresh_after_update() {
    let mut tracker = PlanTracker::new();
    let mut status_messages: Vec<String> = Vec::new();

    // === Phase 1: Plan detection and initial display ===
    let plan_text = "## Task Plan\n1. Read config\n2. Fix bug\n3. Write tests";
    assert!(detect_task_plan(plan_text));

    tracker.parse_plan(plan_text);
    let initial_display = tracker.format_with_confirmation();
    status_messages.push(initial_display.clone());
    tracker.confirm();

    // Initial status_messages should have plan but no checkmarks
    let plan_msg = status_messages
        .iter()
        .find(|m| m.contains("📋 Task Plan"))
        .expect("status_messages should contain plan");
    assert!(
        !plan_msg.contains("✓"),
        "Initial plan in status_messages should have no checkmarks"
    );

    // === Phase 2: After tool call completes step 1 ===
    // Simulate: model appends new text with step 1 ✓
    let mut accumulated_text = format!("{}\nSome tool output\n1. Read config ✓", plan_text);
    tracker.update_from_text(&accumulated_text);

    // This is the critical fix: refresh status_messages after update
    let updated_display = tracker.format_with_confirmation();
    if let Some(plan_msg) = status_messages
        .iter_mut()
        .find(|m| m.contains("📋 Task Plan"))
    {
        *plan_msg = updated_display;
    }

    let plan_msg = status_messages
        .iter()
        .find(|m| m.contains("📋 Task Plan"))
        .expect("status_messages should still contain plan");
    assert!(
        plan_msg.contains("Read config ✓"),
        "After step 1 completion, status_messages should show checkmark. Plan:\n{}",
        plan_msg
    );
    assert_eq!(
        plan_msg.matches("✓").count(),
        1,
        "Should have exactly 1 checkmark"
    );

    // === Phase 3: After tool call completes step 2 ===
    accumulated_text = format!(
        "{}\nSome tool output\n1. Read config ✓\n2. Fix bug ✓",
        plan_text
    );
    tracker.update_from_text(&accumulated_text);

    let updated_display = tracker.format_with_confirmation();
    if let Some(plan_msg) = status_messages
        .iter_mut()
        .find(|m| m.contains("📋 Task Plan"))
    {
        *plan_msg = updated_display;
    }

    let plan_msg = status_messages
        .iter()
        .find(|m| m.contains("📋 Task Plan"))
        .unwrap();
    assert_eq!(
        plan_msg.matches("✓").count(),
        2,
        "Should have 2 checkmarks after step 2. Plan:\n{}",
        plan_msg
    );

    // === Phase 4: FinalResponse — all steps done ===
    accumulated_text = format!(
        "{}\n1. Read config ✓\n2. Fix bug ✓\n3. Write tests ✓",
        plan_text
    );
    tracker.update_from_text(&accumulated_text);

    let final_display = tracker.format_with_confirmation();
    if let Some(plan_msg) = status_messages
        .iter_mut()
        .find(|m| m.contains("📋 Task Plan"))
    {
        *plan_msg = final_display;
    }

    let plan_msg = status_messages
        .iter()
        .find(|m| m.contains("📋 Task Plan"))
        .unwrap();
    assert_eq!(
        plan_msg.matches("✓").count(),
        3,
        "All 3 steps should have checkmarks at final response. Plan:\n{}",
        plan_msg
    );
}

#[test]
fn test_streaming_flow_status_messages_without_refresh_stays_stale() {
    // This demonstrates the BUG scenario: if we don't refresh status_messages
    // after update_from_text, the display stays stale (no checkmarks).
    let mut tracker = PlanTracker::new();
    let mut status_messages: Vec<String> = Vec::new();

    let plan_text = "1. Step A\n2. Step B";
    tracker.parse_plan(plan_text);
    status_messages.push(tracker.format_with_confirmation());
    tracker.confirm();

    // Complete step 1 via update_from_text
    let accumulated = format!("{}\n1. Step A ✓", plan_text);
    tracker.update_from_text(&accumulated);

    // BUG: DON'T refresh status_messages (simulating the old code)
    // The internal tracker state is updated...
    assert_eq!(tracker.current_step_index(), 2);

    // ...but status_messages still shows the old display
    let stale_msg = status_messages
        .iter()
        .find(|m| m.contains("📋 Task Plan"))
        .unwrap();
    assert!(
        !stale_msg.contains("✓"),
        "Without refresh, status_messages stays stale (no checkmarks) — this was the bug"
    );
}

// ============================================================================
// Line splitting logic tests
//
// These tests verify the specific pattern used in render_chat_area to convert
// multi-line status messages into ratatui Line objects:
//   for line in msg.lines() {
//       lines.push(Line::from(line));
//   }
// ============================================================================

#[test]
fn test_plan_display_lines_split_correctly() {
    let mut tracker = PlanTracker::new();
    tracker.parse_plan("## Task Plan\n1. Step one\n2. Step two\n3. Step three");

    let display = tracker.format_with_confirmation();
    let split_lines: Vec<&str> = display.lines().collect();

    // Each step should appear as its own line after splitting
    for (i, step_name) in ["Step one", "Step two", "Step three"].iter().enumerate() {
        let found = split_lines.iter().any(|l| {
            l.contains(&format!("{}. {}", i + 1, step_name))
        });
        assert!(
            found,
            "Step {} '{}' should appear on its own line. All lines: {:?}",
            i + 1,
            step_name,
            split_lines
        );
    }
}

#[test]
fn test_plan_display_lines_with_checkmarks_split_correctly() {
    let mut tracker = PlanTracker::new();
    tracker.parse_plan("1. Alpha\n2. Beta\n3. Gamma");
    tracker.complete_current_step(); // Alpha ✓
    tracker.complete_current_step(); // Beta ✓

    let display = tracker.format_with_confirmation();
    let split_lines: Vec<&str> = display.lines().collect();

    // Verify ✓ lines are on separate lines
    let alpha_line = split_lines
        .iter()
        .find(|l| l.contains("Alpha"))
        .expect("Alpha line should exist");
    assert!(
        alpha_line.contains("✓"),
        "Alpha should have checkmark: {}",
        alpha_line
    );

    let beta_line = split_lines
        .iter()
        .find(|l| l.contains("Beta"))
        .expect("Beta line should exist");
    assert!(
        beta_line.contains("✓"),
        "Beta should have checkmark: {}",
        beta_line
    );

    let gamma_line = split_lines
        .iter()
        .find(|l| l.contains("Gamma"))
        .expect("Gamma line should exist");
    assert!(
        !gamma_line.contains("✓"),
        "Gamma should NOT have checkmark: {}",
        gamma_line
    );
}
