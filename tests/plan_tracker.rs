use my_code_agent::plan_tracker::PlanTracker;

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
