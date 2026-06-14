// Tests for extract_context_from_history - now includes all valid messages with latest highlighted
use my_code_agent::core::agent::review::ReviewAgent;
use my_code_agent::core::types::Message;

#[test]
fn test_extract_context_from_history_single_message() {
    let history = vec![
        Message::user("Add a CSV parser that reads a file and sorts by column"),
        Message::assistant("Here is the code..."),
    ];

    let context = ReviewAgent::extract_context_from_history(&history);

    assert!(
        context.contains("Add a CSV parser"),
        "Single message should be returned directly"
    );
    assert!(
        !context.contains("Conversation History"),
        "Should NOT have Conversation History section for single message"
    );
    assert!(
        !context.contains("Current Task (Primary Focus)"),
        "Should NOT have Current Task section for single message"
    );
}

#[test]
fn test_extract_context_from_history_multi_step_includes_all() {
    let history = vec![
        Message::user("Implement user authentication with JWT and OAuth2"),
        Message::assistant("I'll implement the auth system..."),
        Message::user("Now add database integration"),
        Message::assistant("Adding database layer..."),
        Message::user("Create REST API endpoints"),
    ];

    let context = ReviewAgent::extract_context_from_history(&history);

    // Should contain all historical messages in Conversation History
    assert!(
        context.contains("Implement user authentication"),
        "Should include first historical message, got: {}",
        context
    );
    assert!(
        context.contains("Now add database integration"),
        "Should include second historical message, got: {}",
        context
    );
    // Should contain current task highlighted
    assert!(
        context.contains("Create REST API endpoints"),
        "Should include current task, got: {}",
        context
    );
    // Should have proper format
    assert!(
        context.contains("Conversation History"),
        "Should have Conversation History section"
    );
    assert!(
        context.contains("Current Task (Primary Focus)"),
        "Should have Current Task (Primary Focus) section"
    );
}

#[test]
fn test_extract_context_from_history_skips_fix_prompts() {
    let history = vec![
        Message::user("Implement user authentication"),
        Message::assistant("Here is the auth code..."),
        Message::user("fix the issues found in the code review (iteration 1/3)"),
        Message::assistant("Fixing issues..."),
        Message::user("Auto-Review Iteration 2/3 - Fix Required"),
        Message::assistant("Continuing fixes..."),
        Message::user("Now add error handling for missing files"),
    ];

    let context = ReviewAgent::extract_context_from_history(&history);

    // Should contain original non-fix requirement
    assert!(
        context.contains("Implement user authentication"),
        "Should include original requirement"
    );
    // Should contain current task
    assert!(
        context.contains("add error handling"),
        "Should include current task"
    );
    // Should NOT contain fix prompts
    assert!(
        !context.contains("fix the issues found"),
        "Should filter out fix prompts"
    );
    assert!(
        !context.contains("Auto-Review Iteration"),
        "Should filter out iteration messages"
    );
}

#[test]
fn test_extract_context_from_history_returns_empty_for_only_fix_prompts() {
    let history = vec![
        Message::user("fix the issues found in the code review (iteration 1/3)"),
        Message::assistant("Fixing..."),
    ];

    let context = ReviewAgent::extract_context_from_history(&history);
    assert!(
        context.is_empty(),
        "Should return empty when only fix prompts"
    );
}

#[test]
fn test_extract_context_from_history_preserves_order() {
    let history = vec![
        Message::user("First question about architecture"),
        Message::assistant("Answer 1"),
        Message::user("Second question about database"),
        Message::assistant("Answer 2"),
        Message::user("Third question about testing"),
    ];

    let context = ReviewAgent::extract_context_from_history(&history);

    // Should contain first message as historical
    assert!(
        context.contains("First question"),
        "Should contain first question in history"
    );
    // Should contain second message as historical
    assert!(
        context.contains("Second question"),
        "Should contain second question in history"
    );
    // Should contain last message as current task
    assert!(
        context.contains("Third question"),
        "Should contain third question as current task"
    );
    // Conversation History should come before Current Task
    let history_pos = context.find("Conversation History").unwrap();
    let current_pos = context.find("Current Task (Primary Focus)").unwrap();
    assert!(
        history_pos < current_pos,
        "Conversation History should come before Current Task (Primary Focus)"
    );
}

// Tests for extract_history_summary - now includes all messages with latest highlighted
#[test]
fn test_extract_history_summary_single_turn() {
    let history = vec![
        Message::user("Implement user authentication with JWT"),
        Message::assistant("I'll implement the auth system now..."),
    ];

    let summary = ReviewAgent::extract_history_summary(&history);

    assert!(summary.is_some(), "Should return summary for valid history");
    let summary = summary.unwrap();
    assert!(
        summary.contains("Current Task (Primary Focus)"),
        "Should have Current Task (Primary Focus) section"
    );
    assert!(
        !summary.contains("Conversation History"),
        "Single-turn should NOT have Conversation History"
    );
}

#[test]
fn test_extract_history_summary_multi_step_includes_all() {
    let history = vec![
        Message::user("Build a web application with React frontend and Node.js backend"),
        Message::assistant("I'll start with the project structure..."),
        Message::user("Now add user authentication"),
        Message::assistant("Adding auth system..."),
        Message::user("Create the database models"),
    ];

    let summary = ReviewAgent::extract_history_summary(&history);

    assert!(summary.is_some(), "Should return summary for multi-step");
    let summary = summary.unwrap();
    // Should include all historical messages
    assert!(
        summary.contains("Build a web application"),
        "Should include first historical message, got: {}",
        summary
    );
    assert!(
        summary.contains("Now add user authentication"),
        "Should include second historical message, got: {}",
        summary
    );
    // Should include latest request as primary focus
    assert!(
        summary.contains("Create the database models"),
        "Should include latest request as current task"
    );
    assert!(
        summary.contains("Conversation History"),
        "Should have Conversation History section"
    );
    assert!(
        summary.contains("Current Task (Primary Focus)"),
        "Should have Current Task (Primary Focus) section"
    );
}

#[test]
fn test_extract_history_summary_skips_fix_prompts() {
    let history = vec![
        Message::user("Implement CSV parser"),
        Message::assistant("Here is the parser..."),
        Message::user("fix the issues found in the code review"),
        Message::assistant("Fixing..."),
        Message::user("Add error handling"),
    ];

    let summary = ReviewAgent::extract_history_summary(&history);

    assert!(summary.is_some(), "Should return summary");
    let summary = summary.unwrap();
    // Original should be the first non-fix message
    assert!(
        summary.contains("Implement CSV parser"),
        "Should include original non-fix requirement"
    );
    // Latest should be the last non-fix message
    assert!(
        summary.contains("Add error handling"),
        "Should include latest request"
    );
    // Fix prompts should be filtered
    assert!(
        !summary.contains("fix the issues found"),
        "Should filter fix prompts"
    );
}

#[test]
fn test_extract_history_summary_returns_none_for_empty() {
    let history = vec![];
    assert!(
        ReviewAgent::extract_history_summary(&history).is_none(),
        "Empty history should return None"
    );
}

#[test]
fn test_extract_history_summary_returns_none_for_only_fix_prompts() {
    let history = vec![
        Message::user("fix the issues found in the code review"),
        Message::assistant("Fixing..."),
    ];
    assert!(
        ReviewAgent::extract_history_summary(&history).is_none(),
        "Only fix prompts should return None"
    );
}

#[test]
fn test_extract_history_summary_includes_latest_assistant_reply() {
    let history = vec![
        Message::user("Implement authentication"),
        Message::assistant("I'll implement auth..."),
        Message::user("Add database support"),
        Message::assistant("Adding database integration with PostgreSQL..."),
    ];

    let summary = ReviewAgent::extract_history_summary(&history);

    assert!(summary.is_some());
    let summary = summary.unwrap();
    assert!(
        summary.contains("Adding database integration"),
        "Should include latest assistant reply"
    );
    assert!(
        summary.contains("Latest Assistant Reply"),
        "Should have Latest Assistant Reply section"
    );
}
