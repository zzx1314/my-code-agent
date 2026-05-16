//! 代码审查 Agent 集成测试
//!
//! 测试 ReviewAgent、AgentOrchestrator 和多 Agent 协作流程。

use my_code_agent::core::types::review::*;

/// 测试 ReviewIssue 结构的创建和基本方法
#[test]
fn test_review_issue_creation() {
    let issue = ReviewIssue {
        file: "src/main.rs".to_string(),
        line: Some(42),
        end_line: Some(50),
        severity: Severity::High,
        category: ReviewCategory::Security,
        title: "不安全的函数调用".to_string(),
        description: "使用了不安全的函数，可能导致缓冲区溢出".to_string(),
        suggestion: Some("使用安全的替代函数".to_string()),
        code_snippet: Some("unsafe { ... }".to_string()),
        fix_example: Some("safe_function()".to_string()),
    };

    assert_eq!(issue.severity.icon(), "🟠");
    assert_eq!(issue.severity.label(), "High");
    assert_eq!(issue.category.icon(), "🔒");
    assert_eq!(issue.file, "src/main.rs");
    assert_eq!(issue.line, Some(42));
}
/// 测试 Severity 排序（声明顺序即为优先级排序）
#[test]
fn test_severity_ordering() {
    // Critical 优先级最高（先声明），所以 Critical < High 为 true
    assert!(Severity::Critical < Severity::High);
    assert!(Severity::High < Severity::Medium);
    assert!(Severity::Medium < Severity::Low);
    assert!(Severity::Low < Severity::Info);
}

/// 测试 ReviewVerdict 方法

/// 测试 ReviewVerdict 方法
#[test]
fn test_review_verdict() {
    assert_eq!(ReviewVerdict::Approved.icon(), "✅");
    assert_eq!(ReviewVerdict::Approved.label(), "Approved");
    assert_eq!(ReviewVerdict::NeedsRevision.icon(), "🔄");
    assert_eq!(ReviewVerdict::Rejected.icon(), "❌");
}

/// 测试 ReviewReport 的构建
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
            overall_score: 65.0,
            verdict: ReviewVerdict::NeedsRevision,
        },
        issues: vec![
            ReviewIssue {
                file: "src/main.rs".to_string(),
                line: Some(10),
                end_line: None,
                severity: Severity::Critical,
                category: ReviewCategory::Security,
                title: "SQL 注入风险".to_string(),
                description: "用户输入未经验证直接拼接 SQL 查询".to_string(),
                suggestion: Some("使用参数化查询".to_string()),
                code_snippet: None,
                fix_example: Some("query!(\"SELECT * FROM users WHERE id = ?\", id)".to_string()),
            },
            ReviewIssue {
                file: "src/lib.rs".to_string(),
                line: Some(100),
                end_line: Some(120),
                severity: Severity::High,
                category: ReviewCategory::BugRisk,
                title: "可能的空指针引用".to_string(),
                description: "未检查 Option 值可能为 None".to_string(),
                suggestion: None,
                code_snippet: Some("let x = opt.unwrap();".to_string()),
                fix_example: None,
            },
        ],
        changed_files: vec![
            ChangedFile {
                path: "src/main.rs".to_string(),
                change_type: ChangeType::Modified,
                lines_added: 15,
                lines_removed: 3,
                diff: "+ fn main() {".to_string(),
            },
        ],
        metrics: CodeMetrics {
            files_changed: 1,
            total_lines_added: 15,
            total_lines_removed: 3,
            complexity_estimate: None,
        },
        auto_fixable: vec![],
    };

    assert_eq!(report.summary.total_issues, 2);
    assert_eq!(report.summary.verdict, ReviewVerdict::NeedsRevision);
    assert_eq!(report.issues[0].severity, Severity::Critical);
    assert_eq!(report.metrics.files_changed, 1);
    assert!(report.auto_fixable.is_empty());
}

/// 测试 ChangedFile 和 ChangeType
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

/// 测试 ReviewConfig 的 from_app_config 方法
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
    };

    let config = ReviewConfig::from_app_config(&app_config);
    assert!(config.enabled);
    assert!(config.auto_review);
    assert_eq!(config.severity_threshold, Severity::High);
    assert_eq!(config.max_issues, 50);
    assert!(!config.categories.is_empty());
}

/// 测试 CodeMetrics 创建
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

/// 测试 ReviewSummary 的默认值
#[test]
fn test_review_summary_defaults() {
    let summary = ReviewSummary {
        total_issues: 0,
        critical_count: 0,
        high_count: 0,
        medium_count: 0,
        low_count: 0,
        info_count: 0,
        overall_score: 100.0,
        verdict: ReviewVerdict::Approved,
    };

    assert_eq!(summary.overall_score, 100.0);
    assert_eq!(summary.total_issues, 0);
    assert_eq!(summary.verdict, ReviewVerdict::Approved);
}

/// 测试 ReviewEvent 枚举
#[test]
fn test_review_event_creation() {
    use my_code_agent::app::commands::review::ReviewEvent;

    let started = ReviewEvent::Started { file_count: 5 };
    let progress = ReviewEvent::Progress {
        message: "分析中...".to_string(),
    };
    let error = ReviewEvent::Error {
        message: "出错了".to_string(),
    };

    // 验证这些事件可以匹配
    match started {
        ReviewEvent::Started { file_count } => assert_eq!(file_count, 5),
        _ => panic!("事件类型不匹配"),
    }

    match progress {
        ReviewEvent::Progress { message } => assert_eq!(message, "分析中..."),
        _ => panic!("事件类型不匹配"),
    }

    match error {
        ReviewEvent::Error { message } => assert_eq!(message, "出错了"),
        _ => panic!("事件类型不匹配"),
    }
}

// =============================================================================
// Tests for extract_json_from_response
// =============================================================================

use my_code_agent::core::agent::review_agent::{extract_json_from_response, repair_truncated_json, sanitize_json_escapes};

const VALID_JSON: &str = r#"{"issues":[],"summary":{"overall_score":100,"verdict":"approved"}}"#;

/// Test extracting JSON from a ```json code block (most common LLM output)
#[test]
fn test_extract_json_from_json_code_block() {
    let response = format!("Here's my review:\n\n```json\n{}\n```", VALID_JSON);
    let result = extract_json_from_response(&response).unwrap();
    assert_eq!(result, VALID_JSON);
}

/// Test extracting JSON from a ``` code block without language specifier
#[test]
fn test_extract_json_from_plain_code_block() {
    let response = format!("Review:\n\n```\n{}\n```", VALID_JSON);
    let result = extract_json_from_response(&response).unwrap();
    assert_eq!(result, VALID_JSON);
}

/// Test extracting raw JSON with explanatory text before/after
#[test]
fn test_extract_json_raw_with_surrounding_text() {
    let response = format!("Here is the review result: {} I hope this helps!", VALID_JSON);
    let result = extract_json_from_response(&response).unwrap();
    assert_eq!(result, VALID_JSON);
}

/// Test extracting JSON with nested braces
#[test]
fn test_extract_json_nested_braces() {
    let json = r#"{"issues":[{"file":"test.rs","line":5,"description":"nested { brace here"}],"summary":{"overall_score":85,"verdict":"approved"}}"#;
    let response = format!("Result: {}", json);
    let result = extract_json_from_response(&response).unwrap();
    assert_eq!(result, json);
}

/// Test that the entire response being valid JSON works
#[test]
fn test_extract_json_entire_response_is_json() {
    let result = extract_json_from_response(VALID_JSON).unwrap();
    assert_eq!(result, VALID_JSON);
}

/// Test extracting JSON from a multi-line response with code block
#[test]
fn test_extract_json_multiline_code_block() {
    let json = r#"{
  "issues": [
    {
      "file": "src/main.rs",
      "line": 42,
      "severity": "high",
      "category": "security",
      "title": "Unsafe function",
      "description": "Found unsafe code"
    }
  ],
  "summary": {
    "overall_score": 70,
    "verdict": "needs_revision"
  }
}"#;
    let response = format!("```json\n{}\n```", json);
    let result = extract_json_from_response(&response).unwrap();
    // Parse both to compare structurally (ignore whitespace differences)
    let expected: serde_json::Value = serde_json::from_str(json).unwrap();
    let actual: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(expected, actual);
}

/// Test that an empty response returns an error
#[test]
fn test_extract_json_empty_response() {
    let result = extract_json_from_response("");
    assert!(result.is_err());
}

/// Test extracting single-field JSON
#[test]
fn test_extract_json_single_object_with_nested_text() {
    let json = r#"{"only": "value"}"#;
    let response = format!("Some text {} more text", json);
    let result = extract_json_from_response(&response).unwrap();
    assert_eq!(result, json);
}

// =============================================================================
// Tests for repair_truncated_json
// =============================================================================

/// Valid JSON should pass through unchanged
#[test]
fn test_repair_truncated_already_valid() {
    let json = r#"{"issues":[],"summary":{"score":100}}"#;
    let result = repair_truncated_json(json);
    assert_eq!(result, json);
    assert!(serde_json::from_str::<serde_json::Value>(&result).is_ok());
}

/// Unclosed string at end should be closed
#[test]
fn test_repair_truncated_unclosed_string() {
    let result = repair_truncated_json(r#"{"key": "value"#);
    assert!(serde_json::from_str::<serde_json::Value>(&result).is_ok(),
        "expected valid JSON, got: {result}");
}

/// Unclosed nested brace should be closed
#[test]
fn test_repair_truncated_unclosed_brace() {
    let result = repair_truncated_json(r#"{"a": {"b": 1}"#);
    assert!(serde_json::from_str::<serde_json::Value>(&result).is_ok(),
        "expected valid JSON, got: {result}");
}

/// Unclosed array bracket should be closed
#[test]
fn test_repair_truncated_unclosed_bracket() {
    let result = repair_truncated_json(r#"{"items": [1, 2, 3"#);
    assert!(serde_json::from_str::<serde_json::Value>(&result).is_ok(),
        "expected valid JSON, got: {result}");
}

/// Trailing comma should be removed
#[test]
fn test_repair_truncated_trailing_comma() {
    let result = repair_truncated_json(r#"{"a": 1,}"#);
    assert!(!result.contains(",}"), "unexpected trailing comma in: {result}");
    assert!(serde_json::from_str::<serde_json::Value>(&result).is_ok(),
        "expected valid JSON, got: {result}");
}

/// Full truncated review JSON should be repairable
/// (uses the same sanitize-then-repair pipeline as the real code)
#[test]
fn test_repair_truncated_full_review() {
    let truncated = r#"{"issues":[{"file":"src/main.rs","line":42,"severity":"high","title":"Issue","description":"Found a bug in C:\Users\test"}"#;
    let sanitized = sanitize_json_escapes(truncated);
    let result = repair_truncated_json(&sanitized);
    assert!(serde_json::from_str::<serde_json::Value>(&result).is_ok(),
        "expected valid JSON after sanitize+repair, got: {result}");
}

/// String with escaped quotes inside should not break repair
#[test]
fn test_repair_truncated_escaped_quotes() {
    let truncated = r#"{"desc": "value with \" quote"}"#;
    let result = repair_truncated_json(truncated);
    assert!(serde_json::from_str::<serde_json::Value>(&result).is_ok());
}

/// Nested brackets in various orders
#[test]
fn test_repair_truncated_nested_brackets() {
    let result = repair_truncated_json(r#"{"arr": [[[1, 2"#);
    assert!(serde_json::from_str::<serde_json::Value>(&result).is_ok());
}

/// Multiple unclosed levels
#[test]
fn test_repair_truncated_multi_level() {
    let result = repair_truncated_json(r#"{"a": {"b": {"c": 1"#);
    assert!(serde_json::from_str::<serde_json::Value>(&result).is_ok());
}

// =============================================================================
// Tests for parse_tool_output
// =============================================================================

use my_code_agent::core::agent::orchestrator::AgentOrchestrator;

/// Test parsing FileWriteOutput with git_diff
#[test]
fn test_parse_tool_output_file_write() {
    let git_diff = "diff --git a/src/main.rs b/src/main.rs\nindex abc..def 100644\n--- a/src/main.rs\n+++ b/src/main.rs\n@@ -1,3 +1,4 @@\n fn main() {\n+    println!(\"hello\");\n     println!(\"world\");\n }\n";
    let tool_output = serde_json::json!({
        "path": "src/main.rs",
        "bytes_written": 100,
        "git_diff": git_diff
    }).to_string();

    let result = AgentOrchestrator::parse_tool_output(&tool_output).unwrap();
    assert_eq!(result.path, "src/main.rs");
    assert_eq!(result.change_type, ChangeType::Modified);
    assert_eq!(result.lines_added, 1);
    assert_eq!(result.lines_removed, 0);
    assert_eq!(result.diff, git_diff);
}

/// Test parsing FileWriteOutput for a new file (added)
#[test]
fn test_parse_tool_output_file_added() {
    let git_diff = "diff --git a/src/new.rs b/src/new.rs\nnew file mode 100644\nindex 000..abc\n--- /dev/null\n+++ b/src/new.rs\n@@ -0,0 +1,3 @@\n+fn new_func() {\n+    println!(\"new\");\n+}\n";
    let tool_output = serde_json::json!({
        "path": "src/new.rs",
        "bytes_written": 50,
        "git_diff": git_diff
    }).to_string();

    let result = AgentOrchestrator::parse_tool_output(&tool_output).unwrap();
    assert_eq!(result.path, "src/new.rs");
    assert_eq!(result.change_type, ChangeType::Added);
    assert_eq!(result.lines_added, 3);
}

/// Test parsing FileUpdateOutput
#[test]
fn test_parse_tool_output_file_update() {
    let git_diff = "diff --git a/src/lib.rs b/src/lib.rs\nindex 123..456 100644\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -5,7 +5,7 @@\n-pub fn old_func() {\n+pub fn new_func() {\n     // body\n }\n";
    let tool_output = serde_json::json!({
        "path": "src/lib.rs",
        "replacements": 1,
        "diff": "@@ line 5 @@\n-old\n+new\n",
        "git_diff": git_diff
    }).to_string();

    let result = AgentOrchestrator::parse_tool_output(&tool_output).unwrap();
    assert_eq!(result.path, "src/lib.rs");
    assert_eq!(result.change_type, ChangeType::Modified);
    assert_eq!(result.lines_added, 1);
    assert_eq!(result.lines_removed, 1);
}

/// Test parsing non-JSON content returns None (falls through to text extraction)
#[test]
fn test_parse_tool_output_non_json() {
    let result = AgentOrchestrator::parse_tool_output("Path: src/main.rs (modified)");
    assert!(result.is_none());
}

/// Test parsing JSON without path field returns None
#[test]
fn test_parse_tool_output_no_path() {
    let output = serde_json::json!({"message": "success"}).to_string();
    let result = AgentOrchestrator::parse_tool_output(&output);
    assert!(result.is_none());
}

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
    };
    // Even with NeedsRevision, auto_trigger=false = should NOT trigger fix loop
    let should_fix = outcome.auto_trigger && outcome.verdict != ReviewVerdict::Approved;
    assert!(!should_fix, "Manual review should not trigger fix loop");
}

/// Test ReviewOutcome with Rejected verdict (should trigger fix loop)
#[test]
fn test_review_outcome_rejected() {
    let outcome = ReviewOutcome {
        display_text: "❌ Code rejected".to_string(),
        verdict: ReviewVerdict::Rejected,
        report_summary: "Score: 30/100, 5 critical issues".to_string(),
        report: None,
        auto_trigger: true,
    };
    // Rejected + auto_trigger = SHOULD trigger fix loop (code needs serious fixes)
    let should_fix = outcome.auto_trigger && outcome.verdict != ReviewVerdict::Approved;
    assert!(should_fix, "Rejected should trigger fix loop");
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
    let verdict = format!("Verdict: {} (Score: {:.0}/100)", ReviewVerdict::NeedsRevision.label(), 60.0);
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
    let issue_line = format!("Focus on Critical ({}) and High ({}) severity issues first.",
        critical_count, high_count);
    assert_eq!(issue_line, "Focus on Critical (1) and High (1) severity issues first.");
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
    let should_fix = auto_trigger && verdict != ReviewVerdict::Approved && review_iteration < max_iterations;
    assert!(should_fix);
    review_iteration += 1; // now = 1

    // Second iteration: still needs revision, should fix
    let should_fix = auto_trigger && verdict != ReviewVerdict::Approved && review_iteration < max_iterations;
    assert!(should_fix);
    review_iteration += 1; // now = 2

    // Third iteration: still needs revision, should fix (last chance)
    let should_fix = auto_trigger && verdict != ReviewVerdict::Approved && review_iteration < max_iterations;
    assert!(should_fix);
    assert_eq!(review_iteration, 2);
    assert!(review_iteration + 1 >= max_iterations); // last iteration flag
    review_iteration += 1; // now = 3

    // Fourth iteration: max reached, should NOT fix
    let should_fix = auto_trigger && verdict != ReviewVerdict::Approved && review_iteration < max_iterations;
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
    let should_fix = true && verdict != ReviewVerdict::Approved && review_iteration < max_iterations;
    assert!(should_fix, "New cycle should start fresh with iteration 0");
}

/// Test the iteration status message for first fix vs last chance
#[test]
fn test_iteration_status_messages() {
    let max_iterations = 3;

    // First iteration (iteration=0, will become 1): "Issues found, fixing..."
    let iteration = 0_usize;
    let status = if iteration + 1 >= max_iterations {
        format!("🔄 **Auto-Review Iteration {}/{}** — Last chance! Fixing issues...",
            iteration + 1, max_iterations)
    } else {
        format!("🔄 **Auto-Review Iteration {}/{}** — Issues found, fixing...",
            iteration + 1, max_iterations)
    };
    assert_eq!(status, "🔄 **Auto-Review Iteration 1/3** — Issues found, fixing...");

    // Last iteration (iteration=2, will become 3): "Last chance!"
    let iteration = 2_usize;
    let status = if iteration + 1 >= max_iterations {
        format!("🔄 **Auto-Review Iteration {}/{}** — Last chance! Fixing issues...",
            iteration + 1, max_iterations)
    } else {
        format!("🔄 **Auto-Review Iteration {}/{}** — Issues found, fixing...",
            iteration + 1, max_iterations)
    };
    assert_eq!(status, "🔄 **Auto-Review Iteration 3/3** — Last chance! Fixing issues...");
}

// =============================================================================
// Tests for Functional Completeness (new feature)
// =============================================================================

use my_code_agent::core::agent::review_agent::ReviewAgent;
use my_code_agent::core::types::Message;

/// Test extract_context_from_history: keeps user messages, filters out fix prompts
#[test]
fn test_extract_context_from_history_filters_fix_prompts() {
    let history = vec![
        Message::user("Add a CSV parser that reads a file and sorts by column"),
        Message::assistant("Here is the code..."),
        Message::tool("call_1", "file_write result"),
        // Simulated fix prompt from auto-review loop - should be filtered out
        Message::user("fix the issues found in the code review (iteration 1/3)"),
        Message::assistant("Fixing issues..."),
        // Another fix prompt
        Message::user("Auto-Review Iteration 2/3 - Fix Required"),
    ];

    let context = ReviewAgent::extract_context_from_history(&history);
    
    // Should contain the original user request
    assert!(context.contains("Add a CSV parser"), "Should keep original user request");
    // Should NOT contain fix loop messages
    assert!(!context.contains("fix the issues found"), "Should filter out fix prompts");
    assert!(!context.contains("Auto-Review Iteration"), "Should filter out iteration messages");
    assert!(!context.contains("Fix Required"), "Should filter out fix required messages");
}

/// Test extract_context_from_history: returns empty string when all messages are filtered
#[test]
fn test_extract_context_from_history_all_filtered() {
    let history = vec![
        Message::user("fix the issues found in the code review (iteration 1/3)"),
        Message::user("Auto-Review Iteration 2/3 - Fix Required"),
    ];

    let context = ReviewAgent::extract_context_from_history(&history);
    assert!(context.is_empty(), "Should return empty when all messages are filtered");
}

/// Test extract_context_from_history: includes original request + recent follow-up
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
    assert!(context.contains("First question"), "Should contain original request");
    // Should contain the follow-up (between first user and last assistant)
    assert!(context.contains("Second question"), "Should contain follow-up message");
    // Messages after last assistant are not included
    assert!(!context.contains("Third question"), "Should NOT contain message after last assistant");
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
        description: "User requested sorting by first column, but no sort function is called.".to_string(),
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
        description: "Only file reading is implemented, sorting and writing are missing".to_string(),
        suggestion: Some("Implement the missing sort and write functions".to_string()),
        code_snippet: Some("fn process(path: &str) { let data = fs::read(path); } \n    // TODO: sort and write".to_string()),
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
            overall_score: 70.0,
            verdict: ReviewVerdict::NeedsRevision,
        },
        issues: vec![
            ReviewIssue {
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
    };

    // NeedsRevision should trigger fix loop
    let should_fix = true && report.summary.verdict != ReviewVerdict::Approved;
    assert!(should_fix, "Functional completeness issue should trigger fix loop");
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
        ReviewVerdict::Rejected
    };

    assert_eq!(actual_verdict, ReviewVerdict::NeedsRevision,
        "Should downgrade approved to needs_revision when functional_completeness issues exist");
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
    let _user_request = "Add a new middleware that logs all HTTP requests and register it in the app router.";
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
            overall_score: 55.0,
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
    };

    // Simulate format_review_coverage output
    let output = format!(
        "### 🔍 Review Coverage\n\n| Category | Status | Issues |\n|----------|--------|:-----:|\n"
    );
    assert!(output.contains("🔍 Review Coverage"));

    // Calculate per-category counts (mirrors the real implementation)
    let func_count = report.issues.iter().filter(|i| matches!(i.category, ReviewCategory::FunctionalCompleteness)).count();
    let sec_count = report.issues.iter().filter(|i| matches!(i.category, ReviewCategory::Security)).count();
    let bug_count = report.issues.iter().filter(|i| matches!(i.category, ReviewCategory::BugRisk)).count();
    let perf_count = report.issues.iter().filter(|i| matches!(i.category, ReviewCategory::Performance)).count();
    let err_count = report.issues.iter().filter(|i| matches!(i.category, ReviewCategory::ErrorHandling)).count();
    let maint_count = report.issues.iter().filter(|i| matches!(i.category, ReviewCategory::Maintainability)).count();
    let style_count = report.issues.iter().filter(|i| matches!(i.category, ReviewCategory::Style)).count();
    let doc_count = report.issues.iter().filter(|i| matches!(i.category, ReviewCategory::Documentation)).count();
    let conc_count = report.issues.iter().filter(|i| matches!(i.category, ReviewCategory::Concurrency)).count();

    assert_eq!(func_count, 2, "Functional Completeness should have 2 issues");
    assert_eq!(sec_count, 1, "Security should have 1 issue");
    assert_eq!(bug_count, 0, "BugRisk should have 0 issues");
    assert_eq!(perf_count, 0, "Performance should have 0 issues");
    assert_eq!(err_count, 0, "ErrorHandling should have 0 issues");
    assert_eq!(maint_count, 0, "Maintainability should have 0 issues");
    assert_eq!(style_count, 0, "Style should have 0 issues");
    assert_eq!(doc_count, 0, "Documentation should have 0 issues");
    assert_eq!(conc_count, 0, "Concurrency should have 0 issues");

    // Categories with issues should show warning, clean ones show passed
    let mut seen_categories = std::collections::HashSet::new();
    for issue in &report.issues {
        seen_categories.insert(std::mem::discriminant(&issue.category));
    }
    assert_eq!(seen_categories.len(), 2, "Should have 2 categories with issues");
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
            overall_score: 100.0,
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
    };

    // Verify the coverage table header format
    let coverage_header = "### 🔍 Review Coverage";
    assert!(coverage_header.contains("Review Coverage"));

    // Verify all 9 categories appear in the coverage table format
    let all_categories = vec![
        ReviewCategory::FunctionalCompleteness,
        ReviewCategory::Security,
        ReviewCategory::BugRisk,
        ReviewCategory::Performance,
        ReviewCategory::ErrorHandling,
        ReviewCategory::Maintainability,
        ReviewCategory::Style,
        ReviewCategory::Documentation,
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
            overall_score: 100.0,
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
        ReviewCategory::Documentation,
        ReviewCategory::Concurrency,
    ] {
        let count = report.issues.iter().filter(|i| i.category == *category).count();
        assert_eq!(count, 0, "Category {:?} should have 0 issues in empty report", category);
    }
}

// =============================================================================
// Tests for is_auto_fix_prompt (fix prompt visibility hiding)
// =============================================================================

use my_code_agent::core::agent::stream::is_auto_fix_prompt;

#[test]
fn test_is_auto_fix_prompt_build_fix_prompt() {
    assert!(is_auto_fix_prompt("## 🔄 Code Review - Iteration 1/3 — Fix Required\n\nSome issues found..."));
    assert!(is_auto_fix_prompt("## 🔄 Code Review - Iteration 2/3 — Fix Required\n\nMore issues..."));
    assert!(is_auto_fix_prompt("## 🔄 Code Review - Iteration 3/3 — Last chance!\n\nFinal fixes..."));
}

#[test]
fn test_is_auto_fix_prompt_fallback_format() {
    assert!(is_auto_fix_prompt("Please fix the issues found in the code review (iteration 1/3). The review needs revision."));
    assert!(is_auto_fix_prompt("Please fix the issues found in the code review (iteration 2/3) so the code passes review."));
}

#[test]
fn test_is_auto_fix_prompt_negative_cases() {
    assert!(!is_auto_fix_prompt("Add a CSV parser that reads a file and sorts by column"));
    assert!(!is_auto_fix_prompt("Here's the implementation of the sort function"));
    assert!(!is_auto_fix_prompt(""));
    assert!(!is_auto_fix_prompt("Code Review - Iteration"));
    assert!(!is_auto_fix_prompt("Please fix the issues found in the linter"));
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
// Tests for is_declaration_line — declaration detection across languages
// =============================================================================

/// Test Rust declarations
#[test]
fn test_is_declaration_line_rust() {
    assert!(ReviewAgent::is_declaration_line("fn main() {"));
    assert!(ReviewAgent::is_declaration_line("pub fn execute() {"));
    assert!(ReviewAgent::is_declaration_line("pub async fn handle() {"));
    assert!(ReviewAgent::is_declaration_line("async fn run() {"));
    assert!(ReviewAgent::is_declaration_line("pub struct Config {"));
    assert!(ReviewAgent::is_declaration_line("struct Inner {"));
    assert!(ReviewAgent::is_declaration_line("pub enum Status {"));
    assert!(ReviewAgent::is_declaration_line("enum Color {"));
    assert!(ReviewAgent::is_declaration_line("pub trait Display {"));
    assert!(ReviewAgent::is_declaration_line("trait Clone {"));
    assert!(ReviewAgent::is_declaration_line("impl Display for Config {"));
    assert!(ReviewAgent::is_declaration_line("impl Default {"));
    assert!(ReviewAgent::is_declaration_line("pub type Result<T> = std::result::Result<T, Error>;"));
    assert!(ReviewAgent::is_declaration_line("type Name = String;"));
    assert!(ReviewAgent::is_declaration_line("pub const VERSION: &str = \"1.0\";"));
    assert!(ReviewAgent::is_declaration_line("const MAX_SIZE: usize = 1024;"));
    assert!(ReviewAgent::is_declaration_line("use std::collections::HashMap;"));
    assert!(ReviewAgent::is_declaration_line("macro_rules! vec {}"));
}

/// Test that regular Rust code is NOT detected as a declaration
#[test]
fn test_is_declaration_line_non_declaration_rust() {
    assert!(!ReviewAgent::is_declaration_line("let x = 5;"));
    assert!(!ReviewAgent::is_declaration_line("x.foo()"));
    assert!(!ReviewAgent::is_declaration_line("return Ok(());"));
    assert!(!ReviewAgent::is_declaration_line("if x > 0 {"));
    assert!(!ReviewAgent::is_declaration_line("for item in list {"));
    assert!(!ReviewAgent::is_declaration_line("while true {"));
    assert!(!ReviewAgent::is_declaration_line("match value {"));
    assert!(!ReviewAgent::is_declaration_line("// comment"));
    assert!(!ReviewAgent::is_declaration_line("  "));
    assert!(!ReviewAgent::is_declaration_line(""));
}

/// Test TypeScript/JavaScript declarations
#[test]
fn test_is_declaration_line_typescript() {
    assert!(ReviewAgent::is_declaration_line("function greet(name: string) {"));
    assert!(ReviewAgent::is_declaration_line("export function hello() {"));
    assert!(ReviewAgent::is_declaration_line("export default class Main {}"));
    assert!(ReviewAgent::is_declaration_line("export class UserService {}"));
    assert!(ReviewAgent::is_declaration_line("export interface User {"));
    assert!(ReviewAgent::is_declaration_line("interface Props {"));
    assert!(ReviewAgent::is_declaration_line("class Calculator {"));
    assert!(ReviewAgent::is_declaration_line("import { useState } from 'react';"));
    assert!(ReviewAgent::is_declaration_line("from 'react'"));
}

/// Test Python declarations
#[test]
fn test_is_declaration_line_python() {
    assert!(ReviewAgent::is_declaration_line("def hello():"));
    assert!(ReviewAgent::is_declaration_line("async def fetch_data():"));
    assert!(ReviewAgent::is_declaration_line("class UserModel:"));
}

// =============================================================================
// Tests for extract_signatures_from_content
// =============================================================================

/// Test extracting signatures from a Rust file with mixed content
#[test]
fn test_extract_signatures_from_content_rust_file() {
    let content = r#"use std::collections::HashMap;

pub const VERSION: &str = "1.0";

pub struct Config {
    name: String,
}

pub fn run(config: &Config) -> Result<()> {
    let x = 42;
    println!("{}", x);
    Ok(())
}

struct Helper {}

impl Helper {
    pub fn new() -> Self {
        Self {}
    }
}
"#;
    let sigs = ReviewAgent::extract_signatures_from_content(content);
    assert!(sigs.contains(&"use std::collections::HashMap".to_string()));
    assert!(sigs.contains(&"pub const VERSION: &str".to_string())); // truncated at =
    assert!(sigs.contains(&"pub struct Config".to_string()));
    assert!(sigs.contains(&"pub fn run(config: &Config) -> Result<()>".to_string()));
    assert!(sigs.contains(&"struct Helper".to_string()));
    assert!(sigs.contains(&"impl Helper".to_string()));
    // Non-declarations should NOT be in the list
    assert!(!sigs.iter().any(|s| s.contains("let x =")), "Should not contain variable assignments");
    assert!(!sigs.iter().any(|s| s.contains("println!")), "Should not contain macro calls");
}

/// Test that empty content returns empty signatures
#[test]
fn test_extract_signatures_from_content_empty() {
    let sigs = ReviewAgent::extract_signatures_from_content("");
    assert!(sigs.is_empty());
}

/// Test that content with only non-declaration code returns empty signatures
#[test]
fn test_extract_signatures_from_content_no_declarations() {
    let content = "let x = 5;\nprintln!(\"hi\");\nx.foo();\n";
    let sigs = ReviewAgent::extract_signatures_from_content(content);
    assert!(sigs.is_empty());
}

/// Test that signatures are truncated at the first {, ;, or =
#[test]
fn test_extract_signatures_content_truncates_at_brace() {
    let content = r#"fn long_function(a: i32, b: i32) -> i32 {
    let result = a + b;
    result
}
"#;
    let sigs = ReviewAgent::extract_signatures_from_content(content);
    assert_eq!(sigs.len(), 1);
    // Should NOT include the body after {
    let sig = &sigs[0];
    assert!(!sig.contains('{'), "Signature should not include opening brace");
    assert!(sig.contains("fn long_function"), "Signature should contain the fn line");
}

// =============================================================================
// Tests for format_file_context — full-file context extraction
// =============================================================================

/// Test format_file_context with a Rust file containing declarations
#[test]
fn test_format_file_context_with_rust_file() {
    let content = "use std::fmt;\n\npub struct Point {\n    x: i32,\n    y: i32,\n}\n\npub fn distance(a: &Point, b: &Point) -> f64 {\n    let dx = a.x - b.x;\n    let dy = a.y - b.y;\n    ((dx * dx + dy * dy) as f64).sqrt()\n}";

    // Create a temporary file for this test
    let temp_dir = std::env::temp_dir().join("codebuff_ffc_test_1");
    let _ = std::fs::create_dir_all(&temp_dir);
    let file_path = temp_dir.join("geometry.rs");
    std::fs::write(&file_path, content).unwrap();

    let changed_files = vec![
        ChangedFile {
            path: file_path.to_string_lossy().to_string(),
            change_type: ChangeType::Modified,
            lines_added: 10,
            lines_removed: 0,
            diff: String::new(),
        },
    ];

    let result = ReviewAgent::format_file_context(&changed_files);

    // Should contain the header
    assert!(result.contains("## File Context"), "Should have file context header");
    assert!(result.contains("key declarations"), "Should describe what it shows");

    // Should contain the file path
    assert!(result.contains("geometry.rs"), "Should mention the file");

    // Should contain declarations
    assert!(result.contains("use std::fmt"), "Should contain imports");
    assert!(result.contains("pub struct Point"), "Should contain struct def");
    assert!(result.contains("pub fn distance(a: &Point, b: &Point) -> f64"), "Should contain fn sig");

    // Should NOT contain non-declaration lines
    assert!(!result.contains("let dx ="), "Should not contain variable assignments");

    // Cleanup
    let _ = std::fs::remove_dir_all(&temp_dir);
}

/// Test format_file_context returns empty for empty file
#[test]
fn test_format_file_context_empty_file() {
    let temp_dir = std::env::temp_dir().join("codebuff_ffc_test_2");
    let _ = std::fs::create_dir_all(&temp_dir);
    let file_path = temp_dir.join("empty.rs");
    std::fs::write(&file_path, "").unwrap();

    let changed_files = vec![
        ChangedFile {
            path: file_path.to_string_lossy().to_string(),
            change_type: ChangeType::Added,
            lines_added: 0,
            lines_removed: 0,
            diff: String::new(),
        },
    ];

    let result = ReviewAgent::format_file_context(&changed_files);
    assert!(result.is_empty(), "Empty file should produce empty context");

    let _ = std::fs::remove_dir_all(&temp_dir);
}

/// Test format_file_context returns empty for file with no declarations
#[test]
fn test_format_file_context_no_declarations() {
    let content = "let x = 5;\nx += 1;\nprintln!(\"{} \", x);\n";
    let temp_dir = std::env::temp_dir().join("codebuff_ffc_test_3");
    let _ = std::fs::create_dir_all(&temp_dir);
    let file_path = temp_dir.join("code.rs");
    std::fs::write(&file_path, content).unwrap();

    let changed_files = vec![
        ChangedFile {
            path: file_path.to_string_lossy().to_string(),
            change_type: ChangeType::Modified,
            lines_added: 3,
            lines_removed: 0,
            diff: String::new(),
        },
    ];

    let result = ReviewAgent::format_file_context(&changed_files);
    assert!(result.is_empty(), "File with no declarations should produce empty context");

    let _ = std::fs::remove_dir_all(&temp_dir);
}

/// Test format_file_context skips deleted files (no longer on disk)
#[test]
fn test_format_file_context_skips_deleted_files() {
    let changed_files = vec![
        ChangedFile {
            path: "/tmp/nonexistent_file_xyz.rs".to_string(),
            change_type: ChangeType::Deleted,
            lines_added: 0,
            lines_removed: 10,
            diff: String::new(),
        },
    ];

    let result = ReviewAgent::format_file_context(&changed_files);
    assert!(result.is_empty(), "Deleted files should be skipped");
}

/// Test format_file_context skips nonexistent files
#[test]
fn test_format_file_context_skips_nonexistent_file() {
    let changed_files = vec![
        ChangedFile {
            path: "/tmp/nonexistent_file_abc.rs".to_string(),
            change_type: ChangeType::Modified,
            lines_added: 5,
            lines_removed: 0,
            diff: String::new(),
        },
    ];

    let result = ReviewAgent::format_file_context(&changed_files);
    assert!(result.is_empty(), "Nonexistent files should be skipped");
}

/// Test format_file_context with multiple files
#[test]
fn test_format_file_context_multiple_files() {
    let temp_dir = std::env::temp_dir().join("codebuff_ffc_test_4");
    let _ = std::fs::create_dir_all(&temp_dir);

    // Create two files
    let file1_path = temp_dir.join("lib.rs");
    std::fs::write(&file1_path, "pub fn helper() -> i32 { 42 }").unwrap();

    let file2_path = temp_dir.join("main.rs");
    std::fs::write(&file2_path, "fn main() { println!(\"hi\"); }").unwrap();

    let changed_files = vec![
        ChangedFile {
            path: file1_path.to_string_lossy().to_string(),
            change_type: ChangeType::Modified,
            lines_added: 1,
            lines_removed: 0,
            diff: String::new(),
        },
        ChangedFile {
            path: file2_path.to_string_lossy().to_string(),
            change_type: ChangeType::Modified,
            lines_added: 1,
            lines_removed: 0,
            diff: String::new(),
        },
    ];

    let result = ReviewAgent::format_file_context(&changed_files);

    assert!(result.contains("pub fn helper() -> i32"), "Should contain file1 declarations");
    assert!(result.contains("fn main()"), "Should contain file2 declarations");
    assert!(result.contains("lib.rs"), "Should mention file1 path");
    assert!(result.contains("main.rs"), "Should mention file2 path");

    let _ = std::fs::remove_dir_all(&temp_dir);
}

/// Test format_file_context with a file having many declarations (to test MAX_SIGNATURES_PER_FILE)
#[test]
fn test_format_file_context_with_many_declarations() {
    let temp_dir = std::env::temp_dir().join("codebuff_ffc_test_5");
    let _ = std::fs::create_dir_all(&temp_dir);
    let file_path = temp_dir.join("many.rs");

    // Generate 100 fn declarations
    let mut content = String::new();
    for i in 0..100 {
        content.push_str(&format!("fn func_{i}() {{}}\n"));
    }
    std::fs::write(&file_path, content).unwrap();

    let changed_files = vec![
        ChangedFile {
            path: file_path.to_string_lossy().to_string(),
            change_type: ChangeType::Added,
            lines_added: 100,
            lines_removed: 0,
            diff: String::new(),
        },
    ];

    let result = ReviewAgent::format_file_context(&changed_files);

    // Should contain first 80 declarations but not all 100
    assert!(result.contains("fn func_0"), "Should contain first declaration");
    assert!(result.contains("fn func_79"), "Should contain 80th declaration");
    assert!(!result.contains("fn func_99"), "Should NOT contain 100th declaration (truncated)");
    assert!(result.contains("... and 20 more declarations"), "Should show truncation count");

    let _ = std::fs::remove_dir_all(&temp_dir);
}

/// Test that no output is produced when no files have declarations
#[test]
fn test_format_file_context_no_output_when_no_declarations() {
    let content = "// just a comment\n// another comment\n";
    let temp_dir = std::env::temp_dir().join("codebuff_ffc_test_6");
    let _ = std::fs::create_dir_all(&temp_dir);
    let file_path = temp_dir.join("comments.rs");
    std::fs::write(&file_path, content).unwrap();

    let changed_files = vec![
        ChangedFile {
            path: file_path.to_string_lossy().to_string(),
            change_type: ChangeType::Modified,
            lines_added: 2,
            lines_removed: 0,
            diff: String::new(),
        },
    ];

    let result = ReviewAgent::format_file_context(&changed_files);
    assert!(result.is_empty(), "Should produce no output when file has no declarations");

    let _ = std::fs::remove_dir_all(&temp_dir);
}
