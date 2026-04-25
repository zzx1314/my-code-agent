use my_code_agent::core::streaming::detect_task_plan;

#[test]
fn test_detect_task_plan_with_emoji_header() {
    assert!(detect_task_plan("## 📋 Task Plan\n1. First step"));
}

#[test]
fn test_detect_task_plan_simple_header() {
    assert!(detect_task_plan(
        "## Task Plan\n1. First step\n2. Second step"
    ));
}

#[test]
fn test_detect_task_plan_short_header() {
    assert!(detect_task_plan("## Plan\n1. First step"));
}

#[test]
fn test_detect_task_plan_h3_header() {
    assert!(detect_task_plan("### Plan\n1. First step"));
}

#[test]
fn test_detect_task_plan_no_plan() {
    assert!(!detect_task_plan("Hello, how can I help you?"));
}

#[test]
fn test_detect_task_plan_in_middle_of_text() {
    assert!(detect_task_plan(
        "Let me help you. ## Task Plan\n1. Do this"
    ));
}

#[test]
fn test_detect_task_plan_empty_string() {
    assert!(!detect_task_plan(""));
}

#[test]
fn test_detect_task_plan_plan_in_code_block() {
    // Code blocks shouldn't trigger plan detection as it's not a real header
    assert!(!detect_task_plan("```\n## Task Plan\n```"));
}
