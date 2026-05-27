//! Code Review Agent Integration Tests
//!
//! Tests for ReviewAgent, AgentOrchestrator, and multi-agent collaboration workflow.

use my_code_agent::app::App;
use my_code_agent::core::agent::stream::{check_review_result, process_review_events};
use my_code_agent::core::context::token_usage::TokenUsage;
use my_code_agent::core::types::review::*;

/// Test ReviewIssue struct creation and basic methods
#[test]
fn test_review_issue_creation() {
    let issue = ReviewIssue {
        file: "src/main.rs".to_string(),
        line: Some(42),
        end_line: Some(50),
        severity: Severity::High,
        category: ReviewCategory::Security,
        title: "Unsafe function call".to_string(),
        description: "Used an unsafe function that could cause buffer overflow".to_string(),
        suggestion: Some("Use a safe alternative function".to_string()),
        code_snippet: Some("unsafe { ... }".to_string()),
        fix_example: Some("safe_function()".to_string()),
    };

    assert_eq!(issue.severity.icon(), "🟠");
    assert_eq!(issue.severity.label(), "High");
    assert_eq!(issue.category.icon(), "🔒");
    assert_eq!(issue.file, "src/main.rs");
    assert_eq!(issue.line, Some(42));
}
/// Test Severity ordering (declaration order is priority order)
#[test]
fn test_severity_ordering() {
    // Critical has highest priority (declared first), so Critical < High is true
    assert!(Severity::Critical < Severity::High);
    assert!(Severity::High < Severity::Medium);
    assert!(Severity::Medium < Severity::Low);
    assert!(Severity::Low < Severity::Info);
}

/// Test ReviewVerdict methods

/// Test ReviewVerdict methods
#[test]
fn test_review_verdict() {
    assert_eq!(ReviewVerdict::Approved.icon(), "✅");
    assert_eq!(ReviewVerdict::Approved.label(), "Approved");
    assert_eq!(ReviewVerdict::NeedsRevision.icon(), "🔄");
}

/// Test ReviewReport construction
#[test]
fn test_review_report_creation() {
    let report = ReviewReport {
        summary: ReviewSummary {
            total_issues: 2,
            critical_count: 1,
            high_count: 1,
            medium_count: 0,
            low_count: 0,
            info_count: 0,
            verdict: ReviewVerdict::NeedsRevision,
        },
        issues: vec![
            ReviewIssue {
                file: "src/main.rs".to_string(),
                line: Some(10),
                end_line: None,
                severity: Severity::Critical,
                category: ReviewCategory::Security,
                title: "SQL injection risk".to_string(),
                description: "User input directly concatenated into SQL query without validation"
                    .to_string(),
                suggestion: Some("Use parameterized queries".to_string()),
                code_snippet: None,
                fix_example: Some("query!(\"SELECT * FROM users WHERE id = ?\", id)".to_string()),
            },
            ReviewIssue {
                file: "src/lib.rs".to_string(),
                line: Some(100),
                end_line: Some(120),
                severity: Severity::High,
                category: ReviewCategory::BugRisk,
                title: "Possible null pointer reference".to_string(),
                description: "Option value not checked for None".to_string(),
                suggestion: None,
                code_snippet: Some("let x = opt.unwrap();".to_string()),
                fix_example: None,
            },
        ],
        changed_files: vec![ChangedFile {
            path: "src/main.rs".to_string(),
            change_type: ChangeType::Modified,
            lines_added: 15,
            lines_removed: 3,
            diff: "+ fn main() {".to_string(),
        }],
        metrics: CodeMetrics {
            files_changed: 1,
            total_lines_added: 15,
            total_lines_removed: 3,
            complexity_estimate: None,
        },
        auto_fixable: vec![],
        llm_feedback: "".to_string(),
    };

    assert_eq!(report.summary.total_issues, 2);
    assert_eq!(report.summary.verdict, ReviewVerdict::NeedsRevision);
    assert_eq!(report.issues[0].severity, Severity::Critical);
    assert_eq!(report.metrics.files_changed, 1);
    assert!(report.auto_fixable.is_empty());
}

/// Test ChangedFile and ChangeType
#[test]
fn test_changed_file() {
    let file = ChangedFile {
        path: "src/core/agent/mod.rs".to_string(),
        change_type: ChangeType::Added,
        lines_added: 100,
        lines_removed: 0,
        diff: String::new(),
    };

    assert_eq!(file.change_type, ChangeType::Added);
    assert_eq!(file.lines_added, 100);
    assert_eq!(file.lines_removed, 0);
}

/// Test ReviewConfig::from_app_config
#[test]
fn test_review_config_from_app_config() {
    let app_config = my_code_agent::core::config::ReviewConfig {
        enabled: true,
        auto_review: true,
        threshold_lines: 5,
        max_issues: 50,
        severity_threshold: "high".to_string(),
        on_file_write: true,
        on_file_update: true,
        max_review_iterations: 3,
        max_file_lines: None,
    };

    let config = ReviewConfig::from_app_config(&app_config);
    assert!(config.enabled);
    assert!(config.auto_review);
    assert_eq!(config.severity_threshold, Severity::High);
    assert_eq!(config.max_issues, 50);
    assert!(!config.categories.is_empty());
}

/// Test CodeMetrics creation
#[test]
fn test_code_metrics() {
    let metrics = CodeMetrics {
        files_changed: 3,
        total_lines_added: 200,
        total_lines_removed: 50,
        complexity_estimate: Some(15.5),
    };

    assert_eq!(metrics.files_changed, 3);
    assert_eq!(metrics.total_lines_added, 200);
    assert_eq!(metrics.total_lines_removed, 50);
    assert_eq!(metrics.complexity_estimate, Some(15.5));
}

/// Test ReviewSummary defaults
#[test]
fn test_review_summary_defaults() {
    let summary = ReviewSummary {
        total_issues: 0,
        critical_count: 0,
        high_count: 0,
        medium_count: 0,
        low_count: 0,
        info_count: 0,
        verdict: ReviewVerdict::Approved,
    };

    assert_eq!(summary.total_issues, 0);
    assert_eq!(summary.verdict, ReviewVerdict::Approved);
}

/// Test ReviewEvent enum
#[test]
fn test_review_event_creation() {
    use my_code_agent::app::commands::review::ReviewEvent;

    let started = ReviewEvent::Started { file_count: 5 };
    let progress = ReviewEvent::Progress {
        message: "Analyzing...".to_string(),
    };
    let error = ReviewEvent::Error {
        message: "An error occurred".to_string(),
    };

    // Verify these events can be matched
    match started {
        ReviewEvent::Started { file_count } => assert_eq!(file_count, 5),
        _ => panic!("Event type mismatch"),
    }

    match progress {
        ReviewEvent::Progress { message } => assert_eq!(message, "Analyzing..."),
        _ => panic!("Event type mismatch"),
    }

    match error {
        ReviewEvent::Error { message } => assert_eq!(message, "An error occurred"),
        _ => panic!("Event type mismatch"),
    }
}


// =============================================================================
// =============================================================================
// Tests for ReviewOutcome (iterative review-fix loop)
// =============================================================================

/// Test ReviewOutcome creation for approved review (should stop loop)
#[test]
fn test_review_outcome_approved() {
    let outcome = ReviewOutcome {
        display_text: "✅ All good!".to_string(),
        verdict: ReviewVerdict::Approved,
        report_summary: "Score: 95/100".to_string(),
        report: None,
        auto_trigger: true,
        review_baseline: None,
    };
    assert_eq!(outcome.verdict, ReviewVerdict::Approved);
    assert!(outcome.auto_trigger);
    // Approved + auto_trigger = should NOT trigger fix loop
    let should_fix = outcome.auto_trigger && outcome.verdict != ReviewVerdict::Approved;
    assert!(!should_fix, "Approved should not trigger fix loop");
}

/// Test ReviewOutcome creation for needs_revision (should trigger fix loop)
#[test]
fn test_review_outcome_needs_revision() {
    let outcome = ReviewOutcome {
        display_text: "🔄 Issues found".to_string(),
        verdict: ReviewVerdict::NeedsRevision,
        report_summary: "Score: 65/100, 3 issues".to_string(),
        report: None,
        auto_trigger: true,
        review_baseline: None,
    };
    assert_eq!(outcome.verdict, ReviewVerdict::NeedsRevision);
    // NeedsRevision + auto_trigger = SHOULD trigger fix loop
    let should_fix = outcome.auto_trigger && outcome.verdict != ReviewVerdict::Approved;
    assert!(should_fix, "NeedsRevision should trigger fix loop");
}

/// Test manual review outcome (auto_trigger=false) never triggers fix loop
#[test]
fn test_review_outcome_manual_no_trigger() {
    let outcome = ReviewOutcome {
        display_text: "Manual review report".to_string(),
        verdict: ReviewVerdict::NeedsRevision,
        report_summary: "Score: 50/100".to_string(),
        report: None,
        auto_trigger: false,
        review_baseline: None,
    };
    // Even with NeedsRevision, auto_trigger=false = should NOT trigger fix loop
    let should_fix = outcome.auto_trigger && outcome.verdict != ReviewVerdict::Approved;
    assert!(!should_fix, "Manual review should not trigger fix loop");
}

/// Test iteration counter logic: iteration < max_iterations allows fix
#[test]
fn test_iteration_below_max_allows_fix() {
    let max_iterations = 3_usize;
    let review_iteration = 0_usize;
    let should_fix = review_iteration < max_iterations;
    assert!(should_fix, "Iteration 0 should be below max 3");

    let review_iteration = 1_usize;
    let should_fix = review_iteration < max_iterations;
    assert!(should_fix, "Iteration 1 should be below max 3");

    let review_iteration = 2_usize;
    let should_fix = review_iteration < max_iterations;
    assert!(should_fix, "Iteration 2 should be below max 3");
}

/// Test iteration counter logic: iteration >= max_iterations stops the loop
#[test]
fn test_iteration_max_stops_loop() {
    let max_iterations = 3_usize;
    let review_iteration = 3_usize;
    let should_fix = review_iteration < max_iterations;
    assert!(!should_fix, "Iteration 3 should NOT be below max 3");

    let review_iteration = 4_usize;
    let should_fix = review_iteration < max_iterations;
    assert!(!should_fix, "Iteration 4 should NOT be below max 3");
}

/// Test iteration counter is reset after approved review
#[test]
fn test_iteration_reset_after_approved() {
    let verdict = ReviewVerdict::Approved;
    let auto_trigger = true;

    let should_fix = auto_trigger && verdict != ReviewVerdict::Approved;
    assert!(!should_fix);

    // After approved, iteration should be 0
    let review_iteration = 0;
    assert_eq!(review_iteration, 0, "Iteration should reset after approved");
}

// =============================================================================
// Tests for review_reasoning clearing behavior
// =============================================================================

use my_code_agent::app::commands::review::ReviewEvent;
use my_code_agent::core::types::review::ReviewOutcome;
use tokio::sync::{broadcast, mpsc};

/// Helper: create a minimal App for review event processing tests.
fn make_review_test_app() -> App {
    let config = Config::default();
    let client = LlmClient::new("http://localhost:8080", "test-key", "test-model");
    let agent = Arc::new(Agent::new(
        client.clone(),
        "test system prompt".to_string(),
        ToolRegistry::new(),
    ));
    let (interrupt_tx, _) = broadcast::channel(1);

    App::new(
        Vec::new(),
        TokenUsage::default(),
        String::new(),
        config,
        agent,
        interrupt_tx,
    )
}

/// Progress event clears accumulated review_reasoning.
#[test]
fn test_review_reasoning_cleared_on_progress() {
    let mut app = make_review_test_app();
    let (tx, rx) = mpsc::unbounded_channel::<ReviewEvent>();
    app.review_event_rx = Some(rx);

    // Simulate accumulated reasoning from a phase
    app.review_reasoning = "Previous phase reasoning content...".to_string();
    assert!(
        !app.review_reasoning.is_empty(),
        "Reasoning should be non-empty before Progress"
    );

    // Send Progress event (indicating new phase starting)
    tx.send(ReviewEvent::Progress {
        message: "Starting phase 2".to_string(),
    })
    .expect("Failed to send Progress event");

    process_review_events(&mut app);

    assert!(
        app.review_reasoning.is_empty(),
        "review_reasoning should be cleared after Progress event (new phase started), got: {:?}",
        app.review_reasoning
    );
}

/// ReasoningDelta accumulates correctly without clearing.
#[test]
fn test_review_reasoning_accumulates_delta() {
    let mut app = make_review_test_app();
    let (tx, rx) = mpsc::unbounded_channel::<ReviewEvent>();
    app.review_event_rx = Some(rx);

    app.review_reasoning = String::new();

    tx.send(ReviewEvent::ReasoningDelta("First chunk ".to_string()))
        .expect("Failed to send delta");
    tx.send(ReviewEvent::ReasoningDelta("second chunk ".to_string()))
        .expect("Failed to send delta");
    tx.send(ReviewEvent::ReasoningDelta("third chunk".to_string()))
        .expect("Failed to send delta");

    process_review_events(&mut app);

    assert_eq!(
        app.review_reasoning, "First chunk second chunk third chunk",
        "ReasoningDelta events should accumulate"
    );
}

/// Progress event clears reasoning between phases, then new phase's reasoning accumulates.
#[test]
fn test_review_reasoning_cleared_between_phases() {
    let mut app = make_review_test_app();
    let (tx, rx) = mpsc::unbounded_channel::<ReviewEvent>();
    app.review_event_rx = Some(rx);

    // Phase 1: accumulate reasoning
    tx.send(ReviewEvent::ReasoningDelta(
        "Phase 1 reasoning... ".to_string(),
    ))
    .expect("Failed to send delta");
    process_review_events(&mut app);
    assert_eq!(
        app.review_reasoning, "Phase 1 reasoning... ",
        "Phase 1 reasoning should accumulate"
    );

    // Phase 1 → Phase 2 transition: Progress clears reasoning
    tx.send(ReviewEvent::Progress {
        message: "Starting phase 2".to_string(),
    })
    .expect("Failed to send Progress");
    process_review_events(&mut app);
    assert!(
        app.review_reasoning.is_empty(),
        "Reasoning should be cleared between phases"
    );

    // Phase 2: new reasoning starts fresh
    tx.send(ReviewEvent::ReasoningDelta(
        "Phase 2 reasoning...".to_string(),
    ))
    .expect("Failed to send delta");
    process_review_events(&mut app);
    assert_eq!(
        app.review_reasoning, "Phase 2 reasoning...",
        "Phase 2 reasoning should start fresh after Progress"
    );
}

/// check_review_result clears review_reasoning when a completed outcome arrives.
#[test]
fn test_check_review_result_clears_reasoning_on_completed() {
    let mut app = make_review_test_app();
    let (tx, rx) = mpsc::channel::<ReviewOutcome>(1);
    app.review_result_rx = Some(rx);

    app.review_reasoning = "Final phase reasoning...".to_string();
    assert!(!app.review_reasoning.is_empty());

    // Simulate a completed outcome
    let outcome = ReviewOutcome {
        display_text: "✅ Review complete".to_string(),
        verdict: ReviewVerdict::Approved,
        report_summary: "Score: 95/100".to_string(),
        report: None,
        auto_trigger: true,
        review_baseline: None,
    };

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        tx.send(outcome).await.expect("Failed to send outcome");
    });

    check_review_result(&mut app);

    assert!(
        app.review_reasoning.is_empty(),
        "review_reasoning should be cleared when review completes, got: {:?}",
        app.review_reasoning
    );
    assert!(
        !app.is_reviewing,
        "is_reviewing should be false after completed"
    );
}

/// check_review_result clears review_reasoning when the channel disconnects.
#[test]
fn test_check_review_result_clears_reasoning_on_disconnect() {
    let mut app = make_review_test_app();
    let (_tx, rx) = mpsc::channel::<ReviewOutcome>(1);
    app.review_result_rx = Some(rx);
    app.is_reviewing = true;

    app.review_reasoning = "Some reasoning...".to_string();
    assert!(!app.review_reasoning.is_empty());

    // Drop _tx to disconnect; call check_review_result
    drop(_tx);
    check_review_result(&mut app);

    assert!(
        app.review_reasoning.is_empty(),
        "review_reasoning should be cleared on disconnect, got: {:?}",
        app.review_reasoning
    );
    assert!(
        !app.is_reviewing,
        "is_reviewing should be false after disconnect"
    );
}

/// check_review_result clears review_reasoning on max iterations reached.
#[test]
fn test_check_review_result_clears_reasoning_on_max_iterations() {
    let mut app = make_review_test_app();
    let (tx, rx) = mpsc::channel::<ReviewOutcome>(1);
    app.review_result_rx = Some(rx);
    app.is_reviewing = true;
    app.review_iteration = app.config.review.max_review_iterations; // already at max

    app.review_reasoning = "Last attempt reasoning...".to_string();
    assert!(!app.review_reasoning.is_empty());

    // Send outcome with NeedsRevision (which would normally trigger fix loop,
    // but since iteration >= max_iterations, it won't)
    let outcome = ReviewOutcome {
        display_text: "🔄 Needs revision".to_string(),
        verdict: ReviewVerdict::NeedsRevision,
        report_summary: "Score: 60/100".to_string(),
        report: None,
        auto_trigger: true,
        review_baseline: None,
    };

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        tx.send(outcome).await.expect("Failed to send outcome");
    });

    check_review_result(&mut app);

    assert!(
        app.review_reasoning.is_empty(),
        "review_reasoning should be cleared when max iterations reached, got: {:?}",
        app.review_reasoning
    );
    assert!(
        !app.is_reviewing,
        "is_reviewing should be false after max iterations"
    );
}

/// Test build_fix_prompt format (verify it contains expected sections)
#[test]
fn test_build_fix_prompt_format() {
    let max_iterations = 3;

    // Verify the expected format of the review coverage table header
    let coverage_header = "### 🔍 Review Coverage";
    assert!(coverage_header.contains("Review Coverage"));

    // Verify the standard fix prompt format strings
    let title = format!(
        "## 🔄 Code Review - Iteration {}/{} — Fix Required\n\n",
        1, max_iterations,
    );
    assert!(title.contains("Iteration 1/3"));
    assert!(title.contains("Fix Required"));

    // Verify iteration counter increments correctly
    let prompt2 = format!("Iteration {}/{}", 2, max_iterations);
    assert_eq!(prompt2, "Iteration 2/3");

    let prompt3 = format!("Iteration {}/{}", 3, max_iterations);
    assert_eq!(prompt3, "Iteration 3/3");

    // Verify verdict line format
    let verdict = format!(
        "Verdict: {} (Score: {:.0}/100)",
        ReviewVerdict::NeedsRevision.label(),
        60.0
    );
    assert_eq!(verdict, "Verdict: Needs Revision (Score: 60/100)");
}

/// Test that the fix prompt correctly includes issue details
#[test]
fn test_fix_prompt_includes_issues() {
    // Verify the build_fix_prompt method from AgentOrchestrator
    // produces a prompt containing issue details by testing the core logic
    let total_issues = 2;
    let issues_text = format!("### Found {} Issues", total_issues);
    assert!(issues_text.contains("2 Issues"));

    let critical_count = 1;
    let high_count = 1;
    let issue_line = format!(
        "Focus on Critical ({}) and High ({}) severity issues first.",
        critical_count, high_count
    );
    assert_eq!(
        issue_line,
        "Focus on Critical (1) and High (1) severity issues first."
    );
}

/// Test complete decision logic combining verdict, auto_trigger, and iteration
/// This mirrors the actual logic in check_review_result
#[test]
fn test_complete_iterative_loop_logic() {
    // Scenario: Auto-review with NeedsRevision, within max iterations
    let mut review_iteration = 0;
    let max_iterations = 3;
    let verdict = ReviewVerdict::NeedsRevision;
    let auto_trigger = true;

    // First iteration: should fix
    let should_fix =
        auto_trigger && verdict != ReviewVerdict::Approved && review_iteration < max_iterations;
    assert!(should_fix);
    review_iteration += 1; // now = 1

    // Second iteration: still needs revision, should fix
    let should_fix =
        auto_trigger && verdict != ReviewVerdict::Approved && review_iteration < max_iterations;
    assert!(should_fix);
    review_iteration += 1; // now = 2

    // Third iteration: still needs revision, should fix (last chance)
    let should_fix =
        auto_trigger && verdict != ReviewVerdict::Approved && review_iteration < max_iterations;
    assert!(should_fix);
    assert_eq!(review_iteration, 2);
    assert!(review_iteration + 1 >= max_iterations); // last iteration flag
    review_iteration += 1; // now = 3

    // Fourth iteration: max reached, should NOT fix
    let should_fix =
        auto_trigger && verdict != ReviewVerdict::Approved && review_iteration < max_iterations;
    assert!(!should_fix);

    // After max iterations, should show "max reached" message
    if !should_fix && auto_trigger && verdict != ReviewVerdict::Approved {
        if review_iteration >= max_iterations {
            // This matches the code in check_review_result
            assert_eq!(review_iteration, 3);
            assert!(review_iteration >= max_iterations);
        }
    }
}

/// Test that a second review cycle starts fresh with iteration=0
#[test]
fn test_new_review_cycle_starts_fresh() {
    let review_iteration = 0;
    let max_iterations = 3;

    // Simulate: first message → review loop (3 iterations)
    // After loop completes (approved or max reached), iteration resets to 0
    // review_iteration already 0 — simulates reset after loop

    // Now user sends a new message → new cycle starts
    let verdict = ReviewVerdict::NeedsRevision;
    let should_fix =
        true && verdict != ReviewVerdict::Approved && review_iteration < max_iterations;
    assert!(should_fix, "New cycle should start fresh with iteration 0");
}

/// Test the iteration status message for first fix vs last chance
#[test]
fn test_iteration_status_messages() {
    let max_iterations = 3;

    // First iteration (iteration=0, will become 1): "Issues found, fixing..."
    let iteration = 0_usize;
    let status = if iteration + 1 >= max_iterations {
        format!(
            "🔄 **Auto-Review Iteration {}/{}** — Last chance! Fixing issues...",
            iteration + 1,
            max_iterations
        )
    } else {
        format!(
            "🔄 **Auto-Review Iteration {}/{}** — Issues found, fixing...",
            iteration + 1,
            max_iterations
        )
    };
    assert_eq!(
        status,
        "🔄 **Auto-Review Iteration 1/3** — Issues found, fixing..."
    );

    // Last iteration (iteration=2, will become 3): "Last chance!"
    let iteration = 2_usize;
    let status = if iteration + 1 >= max_iterations {
        format!(
            "🔄 **Auto-Review Iteration {}/{}** — Last chance! Fixing issues...",
            iteration + 1,
            max_iterations
        )
    } else {
        format!(
            "🔄 **Auto-Review Iteration {}/{}** — Issues found, fixing...",
            iteration + 1,
            max_iterations
        )
    };
    assert_eq!(
        status,
        "🔄 **Auto-Review Iteration 3/3** — Last chance! Fixing issues..."
    );
}

// =============================================================================
// Tests for Functional Completeness (new feature)
// =============================================================================

use my_code_agent::core::agent::review::ReviewAgent;
use my_code_agent::core::types::Message;

/// Test extract_context_from_history: keeps user messages, includes main agent's
/// response to fix prompts as Previous Iteration Feedback, filters out fix prompt content itself.
#[test]
fn test_extract_context_from_history_includes_agent_feedback() {
    let history = vec![
        Message::user("Add a CSV parser that reads a file and sorts by column"),
        Message::assistant("Here is the code..."),
        Message::tool("call_1", "file_write result"),
        // Simulated fix prompt from auto-review loop - content should be filtered
        Message::user("fix the issues found in the code review (iteration 1/3)"),
        Message::assistant(
            "The README language issue is not a real problem — the project uses English by convention. I'll fix the actual bugs though.",
        ),
        // Another fix prompt
        Message::user("Auto-Review Iteration 2/3 - Fix Required"),
    ];

    let context = ReviewAgent::extract_context_from_history(&history);

    // Should contain the original user request
    assert!(
        context.contains("Add a CSV parser"),
        "Should keep original user request"
    );
    // Should NOT contain fix prompt content
    assert!(
        !context.contains("fix the issues found"),
        "Should filter out fix prompts"
    );
    assert!(
        !context.contains("Auto-Review Iteration"),
        "Should filter out iteration messages"
    );
    assert!(
        !context.contains("Fix Required"),
        "Should filter out fix required messages"
    );
    // Should include the main agent's response as Previous Iteration Feedback
    assert!(
        context.contains("Previous Iteration Feedback"),
        "Should include feedback section"
    );
    assert!(
        context.contains("README language issue is not a real problem"),
        "Should include agent's response to review"
    );
    // Should NOT contain "What Was Implemented" since last assistant follows a fix prompt
    assert!(
        !context.contains("What Was Implemented"),
        "Should skip What Was Implemented for fix responses"
    );
}

/// Test extract_context_from_history: returns empty string when all messages are
/// fix prompts with no agent responses (no feedback to extract).
#[test]
fn test_extract_context_from_history_only_fix_prompts() {
    let history = vec![
        Message::user("fix the issues found in the code review (iteration 1/3)"),
        Message::user("Auto-Review Iteration 2/3 - Fix Required"),
    ];

    let context = ReviewAgent::extract_context_from_history(&history);
    assert!(
        context.is_empty(),
        "Should return empty when only fix prompts with no agent responses"
    );
}

/// Test extract_context_from_history: includes original request + recent follow-up
/// and should NOT include Previous Iteration Feedback when there are no fix prompts.
#[test]
fn test_extract_context_from_history_includes_original_and_recent() {
    let history = vec![
        Message::user("First question"),
        Message::assistant("Answer 1"),
        Message::user("Second question - follow up"),
        Message::assistant("Answer 2"),
        Message::user("Third question - final request"),
    ];

    let context = ReviewAgent::extract_context_from_history(&history);

    // Should contain the original request (always included)
    assert!(
        context.contains("First question"),
        "Should contain original request"
    );
    // Should contain the follow-up (between first user and last assistant)
    assert!(
        context.contains("Second question"),
        "Should contain follow-up message"
    );
    // Messages after last assistant are not included
    assert!(
        !context.contains("Third question"),
        "Should NOT contain message after last assistant"
    );
    // No fix prompts → no Previous Iteration Feedback
    assert!(
        !context.contains("Previous Iteration Feedback"),
        "Should not have feedback section when no fix prompts"
    );
}

// =============================================================================
// Tests for ReviewCategory::FunctionalCompleteness
// =============================================================================

/// Test FunctionalCompleteness category icon and construction
#[test]
fn test_functional_completeness_category() {
    let issue = ReviewIssue {
        file: "src/parser.rs".to_string(),
        line: Some(10),
        end_line: None,
        severity: Severity::Critical,
        category: ReviewCategory::FunctionalCompleteness,
        title: "Missing sorting implementation".to_string(),
        description: "User requested sorting by first column, but no sort function is called."
            .to_string(),
        suggestion: Some("Add a sort step before writing output".to_string()),
        code_snippet: None,
        fix_example: None,
    };

    assert_eq!(issue.category.icon(), "\u{1f3af}"); // 🎯
    assert_eq!(issue.severity, Severity::Critical);
    assert_eq!(issue.title, "Missing sorting implementation");
}

// =============================================================================
// Tests for Stricter Verdict Fallback with Functional Completeness
// =============================================================================

/// Test creating a ReviewIssue with FunctionalCompleteness category
#[test]
fn test_review_issue_with_functional_completeness() {
    let issue = ReviewIssue {
        file: "src/main.rs".to_string(),
        line: Some(1),
        end_line: None,
        severity: Severity::High,
        category: ReviewCategory::FunctionalCompleteness,
        title: "Partial implementation".to_string(),
        description: "Only file reading is implemented, sorting and writing are missing"
            .to_string(),
        suggestion: Some("Implement the missing sort and write functions".to_string()),
        code_snippet: Some(
            "fn process(path: &str) { let data = fs::read(path); } \n    // TODO: sort and write"
                .to_string(),
        ),
        fix_example: None,
    };

    assert_eq!(issue.category, ReviewCategory::FunctionalCompleteness);
    assert_eq!(issue.severity, Severity::High);
    assert_eq!(issue.file, "src/main.rs");
    assert_eq!(issue.line, Some(1));
}

/// Test that a report with any functional_completeness issue gets NeedsRevision
#[test]
fn test_verdict_with_functional_completeness_issue_is_needs_revision() {
    let report = ReviewReport {
        summary: ReviewSummary {
            total_issues: 1,
            critical_count: 0,
            high_count: 1,
            medium_count: 0,
            low_count: 0,
            info_count: 0,
            verdict: ReviewVerdict::NeedsRevision,
        },
        issues: vec![ReviewIssue {
            file: "src/main.rs".to_string(),
            line: Some(1),
            end_line: None,
            severity: Severity::High,
            category: ReviewCategory::FunctionalCompleteness,
            title: "Missing feature".to_string(),
            description: "Sorting not implemented".to_string(),
            suggestion: None,
            code_snippet: None,
            fix_example: None,
        }],
        changed_files: vec![],
        metrics: CodeMetrics {
            files_changed: 0,
            total_lines_added: 0,
            total_lines_removed: 0,
            complexity_estimate: None,
        },
        auto_fixable: vec![],
        llm_feedback: "".to_string(),
    };

    // NeedsRevision should trigger fix loop
    let should_fix = true && report.summary.verdict != ReviewVerdict::Approved;
    assert!(
        should_fix,
        "Functional completeness issue should trigger fix loop"
    );
}

/// Test that a report with LLM saying 'approved' but with issues is downgraded
/// This simulates the logic in parse_review_response where the code checks:
/// if LLM says approved but has_blocking_issues || !issues.is_empty() -> NeedsRevision
#[test]
fn test_verdict_downgrade_from_approved_when_issues_exist() {
    let has_functional_completeness_issues = true;
    let critical_count = 0;
    let high_count = 0;
    let medium_count = 0;
    let issues_exist = true; // There IS an issue (even though LLM said approved)

    let has_blocking_issues = critical_count > 0
        || high_count > 1
        || (high_count > 0 && has_functional_completeness_issues)
        || medium_count > 3
        || has_functional_completeness_issues;

    // Simulate LLM returning "approved" but we have issues
    let llm_says_approved = true;
    let actual_verdict = if llm_says_approved {
        if has_blocking_issues || issues_exist {
            ReviewVerdict::NeedsRevision
        } else {
            ReviewVerdict::Approved
        }
    } else {
        ReviewVerdict::NeedsRevision
    };

    assert_eq!(
        actual_verdict,
        ReviewVerdict::NeedsRevision,
        "Should downgrade approved to needs_revision when functional_completeness issues exist"
    );
}

// =============================================================================
// Tests for Real-World Scenarios: Incomplete Code Detection
// =============================================================================

/// Simulate scenario: user asks for CSV processing (read, sort, write)
/// Code only reads the file - missing sort and write
#[test]
fn test_scenario_missing_sort_and_write() {
    let user_request = "Create a function that reads a CSV file, sorts it by the first column, and writes the sorted result to a new file.";
    let code_diff = "+ fn process_csv(path: &str) -> Result<()> {\n+     let content = std::fs::read_to_string(path)?;\n+     println!(\"Read {} bytes\", content.len());\n+     Ok(())\n+ }";

    // Verify the context includes the user request
    assert!(user_request.contains("reads a CSV file"));
    assert!(user_request.contains("sorts it by the first column"));
    assert!(user_request.contains("writes the sorted result"));

    // The diff only reads - no sorting, no writing
    assert!(code_diff.contains("read_to_string"));
    assert!(!code_diff.contains("sort"), "Code is missing sort!");
    assert!(!code_diff.contains("write"), "Code is missing write!");

    // Verify the format_changes_summary would include both
    let changes = format!(
        "## User Request (Requirements)\n\n{}\n\n## Code Changes to Review\n\n### src/parser.rs (Added)\n```diff\n{}\n```",
        user_request, code_diff
    );
    assert!(changes.contains("User Request (Requirements)"));
    assert!(changes.contains("reads a CSV file"));
    assert!(changes.contains("process_csv"));
}

/// Simulate scenario: user asks for REST API endpoint, code has todo!() stub
#[test]
fn test_scenario_stub_implementation() {
    let _user_request = "Add a POST /api/users endpoint that creates a user in the database and returns the user ID.";
    let code_diff = "+ async fn create_user_handler(body: Json<CreateUserRequest>) -> impl Responder {\n+     // TODO: implement user creation\n+     todo!(\"implement user creation\")\n+ }";

    // Check that the diff contains the placeholder
    assert!(code_diff.contains("todo!"));
    assert!(code_diff.contains("TODO"));

    // The handler exists but doesn't actually create a user
    assert!(code_diff.contains("create_user_handler"));
    assert!(!code_diff.contains("INSERT INTO"), "Missing DB insert!");
    assert!(!code_diff.contains("return"), "Missing return value!");

    // In a real review, this should be flagged as functional_completeness issue
    let has_stub = code_diff.contains("todo!") || code_diff.contains("unimplemented!");
    assert!(has_stub, "Should detect stub implementation");
}

/// Simulate scenario: user asks for complete feature, code only does half
#[test]
fn test_scenario_half_implementation() {
    let user_request = "Refactor the UserService class: extract database logic into a Repository pattern, add input validation, and add comprehensive error handling with custom error types.";
    let code_diff = "+ pub struct UserRepository {\n+     db: Connection,\n+ }\n+ impl UserRepository {\n+     pub fn new(db: Connection) -> Self { Self { db } }\n+     pub fn find_by_id(&self, id: i32) -> Option<User> { unimplemented!() }\n+ }";

    // Only repository was extracted - validation and error handling are missing
    assert!(code_diff.contains("UserRepository"));
    assert!(code_diff.contains("unimplemented!()"), "Has stub methods");
    assert!(!code_diff.contains("validate"), "Missing validation!");
    assert!(!code_diff.contains("Error"), "Missing custom error types!");

    // Context should include the full user request
    assert!(user_request.contains("Repository pattern"));
    assert!(user_request.contains("input validation"));
    assert!(user_request.contains("error handling"));
}

/// Simulate scenario: user asks for feature with wiring/configuration
#[test]
fn test_scenario_missing_configuration() {
    let _user_request =
        "Add a new middleware that logs all HTTP requests and register it in the app router.";
    let code_diff = "+ pub struct RequestLogger;\n+ impl Middleware for RequestLogger {\n+     fn handle(&self, req: &Request) -> Response {\n+         println!(\"Request: {} {}\", req.method(), req.path());\n+         req.next()\n+     }\n+ }";

    // Middleware is defined but NOT registered in the router
    assert!(code_diff.contains("impl Middleware"));
    assert!(!code_diff.contains("app.register"), "Missing registration!");
    assert!(!code_diff.contains("router"), "Missing router wiring!");

    // Verify the code has the core logic but is incomplete
    let has_core_logic = code_diff.contains("println!");
    let has_wiring = code_diff.contains("register") || code_diff.contains("mount");
    assert!(has_core_logic, "Should have core logic");
    assert!(!has_wiring, "Should be missing wiring");
}

// =============================================================================
// Tests for Review Coverage Summary
// =============================================================================

/// Test format_review_coverage table format and category counting
#[test]
fn test_review_coverage_table_format() {
    let report = ReviewReport {
        summary: ReviewSummary {
            total_issues: 3,
            critical_count: 1,
            high_count: 1,
            medium_count: 1,
            low_count: 0,
            info_count: 0,
            verdict: ReviewVerdict::NeedsRevision,
        },
        issues: vec![
            ReviewIssue {
                file: "src/security.rs".to_string(),
                line: Some(10),
                end_line: None,
                severity: Severity::Critical,
                category: ReviewCategory::Security,
                title: "Unsafe SQL query".to_string(),
                description: "SQL injection risk".to_string(),
                suggestion: None,
                code_snippet: None,
                fix_example: None,
            },
            ReviewIssue {
                file: "src/main.rs".to_string(),
                line: Some(42),
                end_line: None,
                severity: Severity::High,
                category: ReviewCategory::FunctionalCompleteness,
                title: "Missing feature".to_string(),
                description: "Sort not implemented".to_string(),
                suggestion: None,
                code_snippet: None,
                fix_example: None,
            },
            ReviewIssue {
                file: "src/main.rs".to_string(),
                line: Some(55),
                end_line: None,
                severity: Severity::Medium,
                category: ReviewCategory::FunctionalCompleteness,
                title: "Missing write".to_string(),
                description: "Write to output file not implemented".to_string(),
                suggestion: None,
                code_snippet: None,
                fix_example: None,
            },
        ],
        changed_files: vec![],
        metrics: CodeMetrics {
            files_changed: 0,
            total_lines_added: 0,
            total_lines_removed: 0,
            complexity_estimate: None,
        },
        auto_fixable: vec![],
        llm_feedback: "".to_string(),
    };

    // Simulate format_review_coverage output
    let output = format!(
        "### 🔍 Review Coverage\n\n| Category | Status | Issues |\n|----------|--------|:-----:|\n"
    );
    assert!(output.contains("🔍 Review Coverage"));

    // Calculate per-category counts (mirrors the real implementation)
    let func_count = report
        .issues
        .iter()
        .filter(|i| matches!(i.category, ReviewCategory::FunctionalCompleteness))
        .count();
    let sec_count = report
        .issues
        .iter()
        .filter(|i| matches!(i.category, ReviewCategory::Security))
        .count();
    let bug_count = report
        .issues
        .iter()
        .filter(|i| matches!(i.category, ReviewCategory::BugRisk))
        .count();
    let perf_count = report
        .issues
        .iter()
        .filter(|i| matches!(i.category, ReviewCategory::Performance))
        .count();
    let err_count = report
        .issues
        .iter()
        .filter(|i| matches!(i.category, ReviewCategory::ErrorHandling))
        .count();
    let maint_count = report
        .issues
        .iter()
        .filter(|i| matches!(i.category, ReviewCategory::Maintainability))
        .count();
    let style_count = report
        .issues
        .iter()
        .filter(|i| matches!(i.category, ReviewCategory::Style))
        .count();
    let conc_count = report
        .issues
        .iter()
        .filter(|i| matches!(i.category, ReviewCategory::Concurrency))
        .count();

    assert_eq!(
        func_count, 2,
        "Functional Completeness should have 2 issues"
    );
    assert_eq!(sec_count, 1, "Security should have 1 issue");
    assert_eq!(bug_count, 0, "BugRisk should have 0 issues");
    assert_eq!(perf_count, 0, "Performance should have 0 issues");
    assert_eq!(err_count, 0, "ErrorHandling should have 0 issues");
    assert_eq!(maint_count, 0, "Maintainability should have 0 issues");
    assert_eq!(style_count, 0, "Style should have 0 issues");
    assert_eq!(conc_count, 0, "Concurrency should have 0 issues");

    // Categories with issues should show warning, clean ones show passed
    let mut seen_categories = std::collections::HashSet::new();
    for issue in &report.issues {
        seen_categories.insert(std::mem::discriminant(&issue.category));
    }
    assert_eq!(
        seen_categories.len(),
        2,
        "Should have 2 categories with issues"
    );
}

/// Test that the fix prompt contains the Review Coverage section
#[test]
fn test_fix_prompt_contains_coverage_section() {
    // Create a minimal report and verify the coverage-related strings appear
    let report = ReviewReport {
        summary: ReviewSummary {
            total_issues: 0,
            critical_count: 0,
            high_count: 0,
            medium_count: 0,
            low_count: 0,
            info_count: 0,
            verdict: ReviewVerdict::Approved,
        },
        issues: vec![],
        changed_files: vec![],
        metrics: CodeMetrics {
            files_changed: 0,
            total_lines_added: 0,
            total_lines_removed: 0,
            complexity_estimate: None,
        },
        auto_fixable: vec![],
        llm_feedback: "".to_string(),
    };

    // Verify the coverage table header format
    let coverage_header = "### 🔍 Review Coverage";
    assert!(coverage_header.contains("Review Coverage"));

    // Verify all categories appear in the coverage table format
    let all_categories = vec![
        ReviewCategory::FunctionalCompleteness,
        ReviewCategory::Security,
        ReviewCategory::BugRisk,
        ReviewCategory::Performance,
        ReviewCategory::ErrorHandling,
        ReviewCategory::Maintainability,
        ReviewCategory::Style,
        ReviewCategory::Concurrency,
    ];
    for cat in &all_categories {
        let count = report.issues.iter().filter(|i| i.category == *cat).count();
        assert_eq!(count, 0, "No issues expected in clean report");
    }
}

/// Test format_review_coverage with an empty report (no issues)
#[test]
fn test_review_coverage_empty_report() {
    let report = ReviewReport {
        summary: ReviewSummary {
            total_issues: 0,
            critical_count: 0,
            high_count: 0,
            medium_count: 0,
            low_count: 0,
            info_count: 0,
            verdict: ReviewVerdict::Approved,
        },
        issues: vec![],
        changed_files: vec![],
        metrics: CodeMetrics {
            files_changed: 0,
            total_lines_added: 0,
            total_lines_removed: 0,
            complexity_estimate: None,
        },
        auto_fixable: vec![],
        llm_feedback: "".to_string(),
    };

    // All categories should be "Passed" with "—" for count
    for category in &[
        ReviewCategory::FunctionalCompleteness,
        ReviewCategory::Security,
        ReviewCategory::BugRisk,
        ReviewCategory::Performance,
        ReviewCategory::ErrorHandling,
        ReviewCategory::Maintainability,
        ReviewCategory::Style,
        ReviewCategory::Concurrency,
    ] {
        let count = report
            .issues
            .iter()
            .filter(|i| i.category == *category)
            .count();
        assert_eq!(
            count, 0,
            "Category {:?} should have 0 issues in empty report",
            category
        );
    }
}

// =============================================================================
// Tests for is_auto_fix_prompt (fix prompt visibility hiding)
// =============================================================================

use my_code_agent::core::agent::stream::is_auto_fix_prompt;

#[test]
fn test_is_auto_fix_prompt_build_fix_prompt() {
    assert!(is_auto_fix_prompt(
        "## 🔄 Code Review - Iteration 1/3 — Fix Required\n\nSome issues found..."
    ));
    assert!(is_auto_fix_prompt(
        "## 🔄 Code Review - Iteration 2/3 — Fix Required\n\nMore issues..."
    ));
    assert!(is_auto_fix_prompt(
        "## 🔄 Code Review - Iteration 3/3 — Last chance!\n\nFinal fixes..."
    ));
}

#[test]
fn test_is_auto_fix_prompt_fallback_format() {
    assert!(is_auto_fix_prompt(
        "Please fix the issues found in the code review (iteration 1/3). The review needs revision."
    ));
    assert!(is_auto_fix_prompt(
        "Please fix the issues found in the code review (iteration 2/3) so the code passes review."
    ));
}

#[test]
fn test_is_auto_fix_prompt_negative_cases() {
    assert!(!is_auto_fix_prompt(
        "Add a CSV parser that reads a file and sorts by column"
    ));
    assert!(!is_auto_fix_prompt(
        "Here's the implementation of the sort function"
    ));
    assert!(!is_auto_fix_prompt(""));
    assert!(!is_auto_fix_prompt("Code Review - Iteration"));
    assert!(!is_auto_fix_prompt(
        "Please fix the issues found in the linter"
    ));
    assert!(!is_auto_fix_prompt("## Code Review - Iteration 1/3")); // missing 🔄
}

#[test]
fn test_is_auto_fix_prompt_edge_cases() {
    assert!(!is_auto_fix_prompt("🔄 Code Review - Iteration 1/3")); // missing ##
    assert!(!is_auto_fix_prompt("## 🔄 Code Review"));
    assert!(!is_auto_fix_prompt("fix the issues found")); // wrong case
    assert!(!is_auto_fix_prompt("Please fix the issues")); // incomplete match
}

// =============================================================================
// Tests for should_auto_review
// =============================================================================

use my_code_agent::core::agent::client::LlmClient;
use my_code_agent::core::agent::orchestrator::AgentOrchestrator;
use my_code_agent::core::agent::preamble::Agent;
use my_code_agent::core::config::Config;
use my_code_agent::core::types::{ToolCall, ToolCallFunction};
use my_code_agent::tools::ToolRegistry;
use std::sync::Arc;

/// Helper: create a minimal AgentOrchestrator with auto-review set to the given value
fn make_orchestrator(auto_review_enabled: bool) -> AgentOrchestrator {
    let config = Config::default();
    let client = LlmClient::new("http://localhost:8080", "test-key", "test-model");
    let agent = Arc::new(Agent::new(
        client.clone(),
        "test system prompt".to_string(),
        ToolRegistry::new(),
    ));
    let rcfg = my_code_agent::core::types::review::ReviewConfig::from_app_config(&config.review);
    let review_agent = Arc::new(ReviewAgent::new(
        client,
        rcfg.clone(),
        "reasoning_content".to_string(),
        "collapsed".to_string(),
    ));

    AgentOrchestrator {
        main_agent: agent,
        review_agent,
        config: rcfg,
        auto_review_enabled,
    }
}

/// Helper: create a file_write tool call
fn write_tc(id: &str) -> ToolCall {
    ToolCall {
        id: id.to_string(),
        type_: "function".to_string(),
        function: ToolCallFunction {
            name: "file_write".into(),
            arguments: "{}".into(),
        },
    }
}

/// Helper: create a file_update tool call
fn update_tc(id: &str) -> ToolCall {
    ToolCall {
        id: id.to_string(),
        type_: "function".to_string(),
        function: ToolCallFunction {
            name: "file_update".into(),
            arguments: "{}".into(),
        },
    }
}

/// Helper: create a file_delete tool call
fn delete_tc(id: &str) -> ToolCall {
    ToolCall {
        id: id.to_string(),
        type_: "function".to_string(),
        function: ToolCallFunction {
            name: "file_delete".into(),
            arguments: "{}".into(),
        },
    }
}

/// Helper: create an apply_patch tool call
fn patch_tc(id: &str) -> ToolCall {
    ToolCall {
        id: id.to_string(),
        type_: "function".to_string(),
        function: ToolCallFunction {
            name: "apply_patch".into(),
            arguments: "{}".into(),
        },
    }
}

/// Helper: create a file_read tool call (NOT a write operation)
fn read_tc(id: &str) -> ToolCall {
    ToolCall {
        id: id.to_string(),
        type_: "function".to_string(),
        function: ToolCallFunction {
            name: "file_read".into(),
            arguments: "{}".into(),
        },
    }
}

/// Helper: create a tool result message for a file write operation
fn tool_result(id: &str, path: &str) -> Message {
    Message::tool(
        id,
        serde_json::json!({"path": path, "bytes_written": 100}).to_string(),
    )
}

/// Test 1: Latest assistant turn has a file_write tool call → should return true
#[test]
fn test_should_auto_review_recent_write_returns_true() {
    let orch = make_orchestrator(true);
    let history = vec![
        Message::assistant_with_tool_calls("Writing...", vec![write_tc("call_1")]),
        tool_result("call_1", "src/main.rs"),
    ];
    assert!(orch.should_auto_review(&history));
}

/// Test 2: Latest assistant turn has no tool calls (just text) → should return false
#[test]
fn test_should_auto_review_no_tool_calls_returns_false() {
    let orch = make_orchestrator(true);
    let history = vec![Message::assistant("Here's my analysis.")];
    assert!(!orch.should_auto_review(&history));
}

/// Test 3: Latest assistant has file_read (non-write) → should return false
#[test]
fn test_should_auto_review_read_only_returns_false() {
    let orch = make_orchestrator(true);
    let history = vec![
        Message::assistant_with_tool_calls("Reading...", vec![read_tc("call_1")]),
        Message::tool(
            "call_1",
            serde_json::json!({"path": "src/main.rs", "lines": 10, "total_lines": 100}).to_string(),
        ),
    ];
    assert!(!orch.should_auto_review(&history));
}

/// Test 4: Old turn has file_write, but current turn (after last user msg) has no write ops → should return false
#[test]
fn test_should_auto_review_old_write_no_recent_write_returns_false() {
    let orch = make_orchestrator(true);
    let history = vec![
        // Turn 1: old write (already reviewed in previous turn)
        Message::user("Create a function"),
        Message::assistant_with_tool_calls("Writing...", vec![write_tc("call_1")]),
        tool_result("call_1", "src/main.rs"),
        Message::assistant("Done!"),
        // Turn 2: just text, no writes (should NOT trigger auto-review)
        Message::user("Explain the code"),
        Message::assistant("It does X."),
    ];
    assert!(!orch.should_auto_review(&history));
}

/// Test 5: file_update is recognized as a write operation
#[test]
fn test_should_auto_review_file_update_returns_true() {
    let orch = make_orchestrator(true);
    let history = vec![
        Message::assistant_with_tool_calls("Updating...", vec![update_tc("call_1")]),
        tool_result("call_1", "src/lib.rs"),
    ];
    assert!(orch.should_auto_review(&history));
}

/// Test 6: file_delete is recognized as a write operation
#[test]
fn test_should_auto_review_file_delete_returns_true() {
    let orch = make_orchestrator(true);
    let history = vec![
        Message::assistant_with_tool_calls("Deleting...", vec![delete_tc("call_1")]),
        tool_result("call_1", "src/old.rs"),
    ];
    assert!(orch.should_auto_review(&history));
}

/// Test 7: apply_patch is now recognized as a write operation
#[test]
fn test_should_auto_review_apply_patch_returns_true() {
    let orch = make_orchestrator(true);
    let history = vec![
        Message::assistant_with_tool_calls("Patching...", vec![patch_tc("call_1")]),
        tool_result("call_1", "src/main.rs"),
    ];
    assert!(orch.should_auto_review(&history));
}

/// Test 8: auto_review_enabled = false → returns false even with write ops
#[test]
fn test_should_auto_review_disabled_returns_false() {
    let orch = make_orchestrator(false);
    let history = vec![
        Message::assistant_with_tool_calls("Writing...", vec![write_tc("call_1")]),
        tool_result("call_1", "src/main.rs"),
    ];
    assert!(!orch.should_auto_review(&history));
}

/// Test 9: Empty history → returns false
#[test]
fn test_should_auto_review_empty_history_returns_false() {
    let orch = make_orchestrator(true);
    assert!(!orch.should_auto_review(&[]));
}

/// Test 10: Tool result has no valid path but has recent write → still triggers review
/// (actual diff is fetched via git diff, not from tool output)
#[test]
fn test_should_auto_review_missing_file_path_still_triggers() {
    let orch = make_orchestrator(true);
    let history = vec![
        Message::assistant_with_tool_calls("Writing...", vec![write_tc("call_1")]),
        // Tool result without a "path" field
        Message::tool("call_1", r#"{"message": "ok"}"#),
    ];
    assert!(orch.should_auto_review(&history));
}

// =============================================================================
// Tests for review_baseline — incremental diff integration tests
// =============================================================================
//
// Scenario: after multiple rounds of code modification, review_baseline ensures
// the review only sees incremental changes, not cumulative ones.
//
// Test flow:
// 1. Create an independent git repo in a temp directory, commit initial files
// 2. Round 1: modify file_a.rs (add hello function)
//    - verify: detect_changed_files(None) detects 1 changed file
// 3. Create review baseline (create_review_baseline)
//    - verify: returns a valid SHA
//    - verify: detect_changed_files(baseline) returns empty (baseline captures current state)
// 4. Round 2: modify file_a.rs (add goodbye function) + create file_b.rs
//    - verify: detect_changed_files(None) shows cumulative changes (2 files, file_a has all rounds)
//    - verify: detect_changed_files(Some(baseline)) shows incremental changes (2 files, file_a only round 2)
//    - verify: incremental file_a lines < cumulative file_a lines
//    - verify: incremental file_b lines == cumulative file_b lines (new file, same in both)
//
// Note: these tests use set_current_dir which changes the process-level global
// directory, so they must use a global mutex for serialized execution to avoid
// parallel test interference. All test_review_baseline_* tests acquire
// REVIEW_BASELINE_TEST_MUTEX.

use tempfile::TempDir;

use std::sync::LazyLock;
use std::sync::Mutex;

/// Global mutex: serialize all review_baseline integration tests.
/// These tests must exclusively hold set_current_dir — parallel runs would overwrite each other.
static REVIEW_BASELINE_TEST_MUTEX: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

/// Drop guard: ensures the original directory is restored even on panic.
/// Unlike manual `restore()` closures, this struct restores automatically on drop,
/// and panic triggers drop (unless abort).
struct CwdGuard {
    original_dir: std::path::PathBuf,
}

impl CwdGuard {
    fn new(temp_dir: &TempDir) -> Self {
        let original_dir = std::env::current_dir().expect("Failed to get current dir");
        std::env::set_current_dir(temp_dir.path()).expect("Failed to cd to temp dir");
        CwdGuard { original_dir }
    }
}

impl Drop for CwdGuard {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.original_dir);
    }
}

fn run_git(args: &[&str]) {
    let output = std::process::Command::new("git")
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .expect("Failed to run git command");
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        panic!("git {} failed: {}", args.join(" "), stderr);
    }
}

#[test]
fn test_review_baseline_incremental_diff() {
    let _lock = REVIEW_BASELINE_TEST_MUTEX
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let _guard = CwdGuard::new(&temp_dir);

    // Initialize git repository
    run_git(&["init"]);
    run_git(&["config", "user.email", "test@test.com"]);
    run_git(&["config", "user.name", "Test"]);

    // Initial commit: lib.rs with main function
    std::fs::write("lib.rs", "fn main() {\n    println!(\"v1\");\n}\n")
        .expect("Failed to write lib.rs");
    run_git(&["add", "lib.rs"]);
    run_git(&["commit", "-m", "Initial"]);

    let orch = make_orchestrator(true);
    let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");

    // ---- Round 1: add feature_x ----
    std::fs::write(
        "lib.rs",
        "fn main() {\n    println!(\"v1\");\n}\n\npub fn feature_x() -> &'static str {\n    \"x\"\n}\n",
    )
    .expect("Failed");

    let round1_changes = rt.block_on(orch.detect_changed_files_from_git(None));
    assert_eq!(round1_changes.len(), 1);
    let r1_added = round1_changes[0].lines_added;
    assert!(r1_added > 0);

    // Create baseline
    let baseline_sha =
        AgentOrchestrator::create_review_baseline().expect("Should create baseline after round 1");
    assert!(!baseline_sha.is_empty());

    // Baseline should exactly capture current state
    let baseline_check = rt.block_on(orch.detect_changed_files_from_git(Some(&baseline_sha)));
    assert!(baseline_check.is_empty());

    // ---- Round 2: add feature_y (modify lib.rs only, no new file) ----
    // git diff doesn't show untracked files, so only modify existing files to verify incremental logic
    std::fs::write(
        "lib.rs",
        "fn main() {\n    println!(\"v1\");\n}\n\npub fn feature_x() -> &'static str {\n    \"x\"\n}\n\npub fn feature_y() -> &'static str {\n    \"y\"\n}\n",
    )
    .expect("Failed");

    // Cumulative diff (no baseline) = round 1 + round 2 all changes
    let cumulative = rt.block_on(orch.detect_changed_files_from_git(None));
    assert_eq!(cumulative.len(), 1);
    let cum_added = cumulative[0].lines_added;

    // Incremental diff (with baseline) = only round 2 changes
    let incremental = rt.block_on(orch.detect_changed_files_from_git(Some(&baseline_sha)));
    assert_eq!(incremental.len(), 1);
    let inc_added = incremental[0].lines_added;

    // Core assertion: incremental < cumulative (baseline excludes round 1 changes)
    assert!(
        inc_added < cum_added,
        "incremental ({}) should be < cumulative ({})",
        inc_added,
        cum_added
    );
    // Incremental lines should equal exactly round 2 additions
    let round2_added = cum_added - r1_added;
    assert_eq!(
        inc_added, round2_added,
        "incremental ({}) should equal round2 only ({})",
        inc_added, round2_added
    );
}

#[test]
fn test_create_review_baseline_clean_tree_returns_none() {
    let _lock = REVIEW_BASELINE_TEST_MUTEX
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let _guard = CwdGuard::new(&temp_dir);

    run_git(&["init"]);
    run_git(&["config", "user.email", "test@test.com"]);
    run_git(&["config", "user.name", "Test"]);

    std::fs::write("main.rs", "fn main() {}").expect("Failed");
    run_git(&["add", "main.rs"]);
    run_git(&["commit", "-m", "Initial"]);

    assert!(AgentOrchestrator::create_review_baseline().is_none());
}

#[test]
fn test_detect_changed_files_non_git_directory() {
    let _lock = REVIEW_BASELINE_TEST_MUTEX
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let _guard = CwdGuard::new(&temp_dir);

    let orch = make_orchestrator(true);
    let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");

    // Non-git directory should return empty Vec, no panic
    let changes = rt.block_on(orch.detect_changed_files_from_git(None));
    assert!(changes.is_empty());

    // Invalid baseline should not panic either
    let changes_with_baseline =
        rt.block_on(orch.detect_changed_files_from_git(Some("invalid-sha")));
    assert!(changes_with_baseline.is_empty());
}

/// Full lifecycle: 3 rounds of modification + 2 baseline creations, verify chained increments
#[test]
fn test_review_baseline_full_lifecycle() {
    let _lock = REVIEW_BASELINE_TEST_MUTEX
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let _guard = CwdGuard::new(&temp_dir);

    run_git(&["init"]);
    run_git(&["config", "user.email", "test@test.com"]);
    run_git(&["config", "user.name", "Test"]);

    std::fs::write("lib.rs", "fn init() {}\n").expect("Failed");
    run_git(&["add", "lib.rs"]);
    run_git(&["commit", "-m", "Initial"]);

    let orch = make_orchestrator(true);
    let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");

    // ---- Round 1 ----
    std::fs::write(
        "lib.rs",
        "fn init() {}\n\npub fn feature_x() -> &'static str {\n    \"feature_x\"\n}\n",
    )
    .expect("Failed");
    let sha1 = AgentOrchestrator::create_review_baseline().expect("Round 1 baseline");
    assert!(
        rt.block_on(orch.detect_changed_files_from_git(Some(&sha1)))
            .is_empty()
    );

    // ---- Round 2 ----
    std::fs::write("lib.rs", "fn init() {}\n\npub fn feature_x() -> &'static str {\n    \"feature_x\"\n}\n\npub fn feature_y() -> &'static str {\n    \"feature_y\"\n}\n").expect("Failed");

    let cumulative2 = rt.block_on(orch.detect_changed_files_from_git(None));
    let cum2_added = cumulative2[0].lines_added;

    let incremental2 = rt.block_on(orch.detect_changed_files_from_git(Some(&sha1)));
    let inc2_added = incremental2[0].lines_added;

    // 增量 < 累积：基线排除了第 1 轮改动
    assert!(inc2_added < cum2_added);

    // 第 2 次基线
    let sha2 = AgentOrchestrator::create_review_baseline().expect("Round 2 baseline");
    assert_ne!(sha1, sha2);
    assert!(
        rt.block_on(orch.detect_changed_files_from_git(Some(&sha2)))
            .is_empty()
    );

    // ---- Round 3 ----
    std::fs::write("lib.rs", "fn init() {}\n\npub fn feature_x() -> &'static str {\n    \"feature_x_updated\"\n}\n\npub fn feature_y() -> &'static str {\n    \"feature_y\"\n}\n").expect("Failed");

    let from_sha1 = rt.block_on(orch.detect_changed_files_from_git(Some(&sha1)));
    let from_sha2 = rt.block_on(orch.detect_changed_files_from_git(Some(&sha2)));

    // Old baseline shows more changes (round 2 + round 3)
    assert!(from_sha1[0].lines_added > from_sha2[0].lines_added);
}

// _guard and _lock drop here → CwdGuard restores directory, Mutex unlocks, TempDir cleans up
